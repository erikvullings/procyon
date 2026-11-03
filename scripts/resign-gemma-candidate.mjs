import { execFileSync, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { semanticWorkerChanges } from './check-desktop-gemma-release.mjs';
import { riskAcceptedGemmaManifestPath } from './gemma-risk-acceptance.mjs';
import { prepareRiskAcceptedGemmaManifest } from './prepare-risk-accepted-gemma-manifest.mjs';

/**
 * One-command version of "Re-signing the risk-accepted Gemma worker candidate"
 * (docs/semantic-operations.md). Every step of that procedure is kept; only the manual hand-offs go.
 *
 *   node scripts/resign-gemma-candidate.mjs --approve-risk-accepted
 *
 * It exits early (changing nothing) when the worker build inputs are unchanged since the signed
 * candidate, because then the desktop gate already passes. The risk acceptance stays an explicit
 * flag: the release owner who runs this, or tells an agent to, is the one accepting it.
 */

const WORKFLOW = 'release-semantic-components.yml';
const MAX_FLAKE_RERUNS = 2;
const POLL_MS = 30_000;

/** `semantic-v0.7.5` -> `semantic-v0.7.6`; anything without a trailing number gets `.1` appended. */
export function nextSemanticTag(tag) {
  const match = /^(.*?)(\d+)$/u.exec(tag);
  return match === null ? `${tag}.1` : `${match[1]}${Number(match[2]) + 1}`;
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
  const options = { approve: false, tag: undefined, qualificationRun: undefined };
  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    if (argument === '--approve-risk-accepted') options.approve = true;
    else if (argument === '--tag') options.tag = argv[++index];
    else if (argument === '--qualification-run') options.qualificationRun = argv[++index];
    else throw new Error(`unknown argument: ${argument}`);
  }
  if (!options.approve) {
    throw new Error(
      'refusing to run without --approve-risk-accepted: this records the release owner’s ' +
        'risk acceptance of the unqualified Gemma worker in docs/evaluations',
    );
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

async function main() {
  const options = parseArguments(process.argv.slice(2));
  const head = requireCleanMain();
  const current = JSON.parse(fs.readFileSync(riskAcceptedGemmaManifestPath, 'utf8'));

  const changed = semanticWorkerChanges(current.sourceRevision, head, process.cwd());
  if (changed.length === 0) {
    log(
      `worker build inputs unchanged since ${current.sourceRevision.slice(0, 8)}; no re-sign needed`,
    );
    return;
  }
  log(`worker inputs changed since the signed candidate:\n  ${changed.join('\n  ')}`);

  const tag = options.tag ?? nextSemanticTag(current.releaseTag);
  if (
    gh(['release', 'list', '--limit', '100', '--json', 'tagName', '-q', '.[].tagName'])
      .split('\n')
      .includes(tag)
  ) {
    throw new Error(`${tag} is already published (immutable): pass a new --tag`);
  }

  // 1. Qualify the candidate on the exact HEAD revision.
  let qualificationRun = options.qualificationRun;
  if (qualificationRun === undefined) {
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
  const manifest = prepareRiskAcceptedGemmaManifest(
    fs.readFileSync(path.join(directory, 'semantic-component-release.json')),
    {
      modelRevision: current.riskAcceptedGemma.modelRevision,
      approvedBy: current.riskAcceptedGemma.approvedBy,
      approvedAt: new Date().toISOString().replace(/\.\d+Z$/u, 'Z'),
    },
  );
  if (manifest.sourceRevision !== head) {
    throw new Error(
      `candidate was built from ${manifest.sourceRevision.slice(0, 8)}, not HEAD ${head.slice(0, 8)}: ` +
        'main moved during qualification, start over',
    );
  }
  fs.writeFileSync(riskAcceptedGemmaManifestPath, `${JSON.stringify(manifest, null, 2)}\n`);

  // 3. Commit (touches no worker build input) and push.
  sh('git', ['add', riskAcceptedGemmaManifestPath]);
  sh('git', [
    'commit',
    '-q',
    '-m',
    `chore(semantic): re-sign the risk-accepted Gemma candidate for ${tag}\n\nApproved by ${manifest.riskAcceptedGemma.approvedBy}.`,
  ]);
  push();
  const signedCommit = sh('git', ['rev-parse', 'HEAD']).trim();

  // 4. Publish the already-qualified run under the same immutable tag.
  const publishRun = await dispatch({
    release_tag: tag,
    qualification_run_id: qualificationRun,
    approve_risk_accepted_gemma: 'true',
  });
  await waitForSuccess(publishRun, 'publish');
  log(`published ${tag}.`);
  log(`tag the desktop release on ${signedCommit} (a descendant of the new sourceRevision).`);
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  main().catch((error) => {
    process.stderr.write(`[resign] ${error.message}\n`);
    process.exit(1);
  });
}
