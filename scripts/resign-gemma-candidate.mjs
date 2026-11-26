import { execFileSync, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import {
  semanticWorkerChanges,
  verifySemanticWorkerSource,
} from './check-desktop-gemma-release.mjs';
import { optInGemmaManifestPath } from './gemma-opt-in.mjs';
import { prepareOptInGemmaManifest } from './prepare-opt-in-gemma-manifest.mjs';
import { nextSemanticTag } from './semantic-release-tags.mjs';

export { nextSemanticTag };

/**
 * One-command version of "Re-signing the opt-in Gemma worker candidate"
 * (docs/semantic-operations.md). Every step of that procedure is kept; only the manual hand-offs go.
 *
 *   node scripts/resign-gemma-candidate.mjs
 *
 * When the current candidate covers HEAD, the script only publishes it if publication is
 * still pending. It carries forward the model approval:
 * the model revision, `approvedBy` and `approvedAt` carry over from the current manifest. A model
 * change requires a new model approval and goes through prepare-opt-in-gemma-manifest.mjs instead.
 */

const WORKFLOW = 'release-semantic-components.yml';
const MAX_FLAKE_RERUNS = 2;
const POLL_MS = 30_000;

/**
 * The immutable tag the manifest and publication must use. A qualification run's tag is baked
 * into its signed catalog URLs, so a resumed run (`--qualification-run`, including one started by
 * a push to main) dictates the tag; a fresh run takes the next number in the series.
 */
export function resolveReleaseTag({ explicitTag, candidateTag, currentTag }) {
  if (candidateTag !== undefined) {
    if (explicitTag !== undefined && explicitTag !== candidateTag) {
      throw new Error(
        `--tag ${explicitTag} differs from the qualified candidate's tag ${candidateTag}`,
      );
    }
    return candidateTag;
  }
  return explicitTag ?? nextSemanticTag(currentTag);
}

/** The newest workflow_dispatch run created at or after `since` (ms epoch), if any. */
export function newestRunSince(runs, since) {
  return runs
    .filter((run) => Date.parse(run.createdAt) >= since)
    .sort((a, b) => Date.parse(b.createdAt) - Date.parse(a.createdAt))[0];
}

function sh(command, args, options = {}) {
  return execFileSync(command, args, {
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'inherit'],
    ...options,
  });
}

function gh(args) {
  return sh('gh', args).trim();
}

function log(message) {
  process.stdout.write(`[resign] ${message}\n`);
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function parseArguments(argv) {
  const options = { tag: undefined, qualificationRun: undefined };
  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    if (argument === '--tag') options.tag = argv[++index];
    else if (argument === '--qualification-run') options.qualificationRun = argv[++index];
    else throw new Error(`unknown argument: ${argument}`);
  }
  return options;
}

/** Polls a run to completion, riding out transient GitHub API failures. */
async function waitForRun(runId) {
  for (;;) {
    try {
      const run = JSON.parse(gh(['run', 'view', String(runId), '--json', 'status,conclusion']));
      if (run.status === 'completed') return run.conclusion;
    } catch {
      // A 5xx from the API says nothing about the run: keep polling.
    }
    await sleep(POLL_MS);
  }
}

/** Waits for a run, re-running only its failed jobs a bounded number of times (runner flakes). */
async function waitForSuccess(runId, label) {
  for (let attempt = 0; ; attempt += 1) {
    log(`${label}: waiting for run ${runId}`);
    const conclusion = await waitForRun(runId);
    if (conclusion === 'success') return;
    if (attempt >= MAX_FLAKE_RERUNS) {
      throw new Error(`${label}: run ${runId} ended ${conclusion} after ${attempt} reruns`);
    }
    log(
      `${label}: run ${runId} ended ${conclusion}; re-running failed jobs (${attempt + 1}/${MAX_FLAKE_RERUNS})`,
    );
    gh(['run', 'rerun', String(runId), '--failed']);
    await sleep(15_000);
  }
}

async function dispatch(fields) {
  const since = Date.now() - 5_000;
  gh([
    'workflow',
    'run',
    WORKFLOW,
    '--ref',
    'main',
    ...Object.entries(fields).flatMap(([key, value]) => ['-f', `${key}=${value}`]),
  ]);
  for (let attempt = 0; attempt < 20; attempt += 1) {
    await sleep(5_000);
    const runs = JSON.parse(
      gh([
        'run',
        'list',
        '--workflow',
        WORKFLOW,
        '--limit',
        '10',
        '--json',
        'databaseId,createdAt,event',
      ]),
    ).filter((run) => run.event === 'workflow_dispatch');
    const run = newestRunSince(runs, since);
    if (run !== undefined) return run.databaseId;
  }
  throw new Error('dispatched run did not appear');
}

function requireCleanMain() {
  const branch = sh('git', ['branch', '--show-current']).trim();
  if (branch !== 'main') throw new Error(`must run on main (on ${branch})`);
  if (sh('git', ['status', '--porcelain']).trim() !== '') {
    throw new Error('working tree is not clean: commit or stash first');
  }
  sh('git', ['fetch', '--quiet', 'origin', 'main']);
  const local = sh('git', ['rev-parse', 'HEAD']).trim();
  const remote = sh('git', ['rev-parse', 'origin/main']).trim();
  if (local !== remote) throw new Error('HEAD is not the pushed origin/main: push or pull first');
  return local;
}

function push() {
  // Same override CLAUDE.md documents: the keyring-backed gh account, not an injected GH_TOKEN.
  const env = { ...process.env };
  delete env.GH_TOKEN;
  const result = spawnSync(
    'git',
    [
      '-c',
      'credential.helper=',
      '-c',
      'credential.helper=!gh auth git-credential',
      'push',
      'origin',
      'main',
    ],
    { stdio: 'inherit', env },
  );
  if (result.status !== 0) throw new Error('git push failed');
}

export function isSemanticReleasePublished(tag) {
  try {
    const release = JSON.parse(
      sh('gh', ['api', `repos/{owner}/{repo}/releases/tags/${tag}`], {
        stdio: ['ignore', 'pipe', 'pipe'],
      }),
    );
    return release.draft === false;
  } catch (error) {
    if (String(error.stderr).includes('HTTP 404')) return false;
    throw error;
  }
}

async function publishCandidate(tag, qualificationRun) {
  const publishRun = await dispatch({
    release_tag: tag,
    qualification_run_id: String(qualificationRun),
    publish_opt_in_gemma: 'true',
  });
  await waitForSuccess(publishRun, 'publish');
  if (!isSemanticReleasePublished(tag)) {
    throw new Error(
      `${tag} was not published by run ${publishRun}; check the semantic publication job and release variables`,
    );
  }
  log(`published ${tag}.`);
}

async function main() {
  const options = parseArguments(process.argv.slice(2));
  const head = requireCleanMain();
  const current = JSON.parse(fs.readFileSync(optInGemmaManifestPath, 'utf8'));

  const changed = semanticWorkerChanges(current.sourceRevision, head, process.cwd());
  if (changed.length === 0) {
    verifySemanticWorkerSource(current.sourceRevision, head, process.cwd());
    if (!isSemanticReleasePublished(current.releaseTag)) {
      await publishCandidate(current.releaseTag, current.qualificationRunId);
    } else {
      log(`worker build inputs match published candidate ${current.releaseTag}`);
    }
    log(`tag the desktop release on ${head}.`);
    return;
  }
  log(`worker inputs changed since the signed candidate:\n  ${changed.join('\n  ')}`);

  const assertTagFree = (tag) => {
    if (isSemanticReleasePublished(tag)) {
      throw new Error(`${tag} is already published (immutable): pass a new --tag`);
    }
  };

  // 1. Qualify the candidate (a fresh run on HEAD) or resume one that already succeeded, such as
  //    the run a push to main started automatically.
  let qualificationRun = options.qualificationRun;
  let tag = resolveReleaseTag({ explicitTag: options.tag, currentTag: current.releaseTag });
  if (qualificationRun === undefined) {
    assertTagFree(tag);
    qualificationRun = String(await dispatch({ release_tag: tag, qualify_gemma: 'true' }));
    log(`qualification dispatched as run ${qualificationRun}`);
  }
  await waitForSuccess(qualificationRun, 'qualification');

  // 2. Build the manifest from the candidate artifact (same inputs the manual procedure uses).
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'procyon-resign-'));
  const artifact = JSON.parse(
    gh(['api', `repos/{owner}/{repo}/actions/runs/${qualificationRun}/artifacts`]),
  ).artifacts.find((entry) => entry.name.startsWith('semantic-component-candidate-'));
  if (artifact === undefined)
    throw new Error('candidate artifact missing from the qualification run');
  gh(['run', 'download', qualificationRun, '-n', artifact.name, '-D', directory]);
  const candidateBytes = fs.readFileSync(path.join(directory, 'semantic-component-release.json'));
  tag = resolveReleaseTag({
    explicitTag: options.tag,
    candidateTag: JSON.parse(candidateBytes).releaseTag,
    currentTag: current.releaseTag,
  });
  assertTagFree(tag);
  const manifest = prepareOptInGemmaManifest(candidateBytes, {
    modelRevision: current.optInGemma.modelRevision,
    approvedBy: current.optInGemma.approvedBy,
    approvedAt: current.optInGemma.approvedAt,
  });
  // The desktop gate's own condition: the candidate must be an ancestor of HEAD with no worker
  // build input changed since. HEAD itself may be newer (main moved, or the run was automatic).
  verifySemanticWorkerSource(manifest.sourceRevision, head, process.cwd());
  fs.writeFileSync(optInGemmaManifestPath, `${JSON.stringify(manifest, null, 2)}\n`);

  // 3. Commit (touches no worker build input) and push.
  sh('git', ['add', optInGemmaManifestPath]);
  sh('git', [
    'commit',
    '-q',
    '-m',
    `chore(semantic): re-sign the opt-in Gemma candidate for ${tag}\n\nApproved by ${manifest.optInGemma.approvedBy}.`,
  ]);
  push();
  const signedCommit = sh('git', ['rev-parse', 'HEAD']).trim();

  // 4. Publish the already-qualified run under the same immutable tag.
  await publishCandidate(tag, qualificationRun);
  log(`tag the desktop release on ${signedCommit} (a descendant of the new sourceRevision).`);
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  main().catch((error) => {
    process.stderr.write(`[resign] ${error.message}\n`);
    process.exit(1);
  });
}
