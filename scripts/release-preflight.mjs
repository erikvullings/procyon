import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import { fileURLToPath } from 'node:url';

import {
  checkDesktopGemmaRelease,
  experimentalManifestPath,
  semanticWorkerChanges,
} from './check-desktop-gemma-release.mjs';
import {
  checkQualifiedGemmaDesktopRelease,
  qualifiedGemmaManifestPath,
} from './check-qualified-gemma-release.mjs';
import {
  checkRiskAcceptedGemmaDesktopRelease,
  riskAcceptedGemmaManifestPath,
} from './gemma-risk-acceptance.mjs';

/**
 * Tells, before a desktop tag is pushed, whether `release-desktop.yml` would pass every gate that
 * does not need a build, and for each failure which command fixes it. It runs the workflow's own
 * gate functions (never a re-implementation), so a pass here is a pass there. Read-only: it
 * fetches `origin/main` and reads GitHub repository variables and runs, and changes nothing.
 *
 *   pnpm release:preflight [--tag v0.7.0-16] [--revision <sha>] [--mode risk-accepted]
 *                          [--no-fetch] [--no-gh]
 */

const repoRoot = fileURLToPath(new URL('..', import.meta.url));
const qualificationWorkflow = 'release-semantic-components.yml';
const desktopTarget = 'macos-aarch64';

export const gemmaModes = {
  experimental: {
    variable: 'SEMANTIC_GEMMA_DESKTOP_EXPERIMENTAL',
    manifestPath: experimentalManifestPath,
  },
  qualified: {
    variable: 'SEMANTIC_GEMMA_NON_EXPERIMENTAL_APPROVED',
    manifestPath: qualifiedGemmaManifestPath,
  },
  'risk-accepted': {
    variable: 'SEMANTIC_GEMMA_RISK_ACCEPTED',
    manifestPath: riskAcceptedGemmaManifestPath,
  },
};

/** The workspace release version, which `check-desktop-release.mjs` requires the tag to match. */
export function workspaceVersion(rootManifestText) {
  const section = /^\[workspace\.package\]\s*$([\s\S]*?)(?=^\[|(?![\s\S]))/mu.exec(
    rootManifestText,
  )?.[1];
  const version = /^version\s*=\s*"([^"]+)"/mu.exec(section ?? '')?.[1];
  if (version === undefined) throw new Error('[workspace.package] version not found');
  return version;
}

/** Gemma modes the workflow would activate, from the repository variables it reads. */
export function activeGemmaModes(vars) {
  return Object.entries(gemmaModes)
    .filter(([, { variable }]) => vars?.[variable] === 'true')
    .map(([mode]) => mode);
}

/** Classifies a gate failure message so the report can name the exact remedy. */
export function classifyGateFailure(message) {
  if (message.startsWith('semantic worker build inputs changed')) return 'worker-changed';
  if (message.includes('not an ancestor')) return 'not-ancestor';
  if (/independent|owner approvals/u.test(message)) return 'variables';
  return 'manifest';
}

/**
 * Picks the push-started qualification run that can stand in for a fresh one: successful (or still
 * running) on a commit that is an ancestor of the release source with no worker build input
 * changed since, which is exactly the desktop gate's own condition.
 */
export function classifyQualificationRuns(runs, revision, { isAncestor, changesBetween }) {
  const covering = runs
    .filter((run) => run.event === 'push')
    .sort((a, b) => Date.parse(b.createdAt) - Date.parse(a.createdAt))
    .filter((run) => {
      try {
        return (
          isAncestor(run.headSha, revision) && changesBetween(run.headSha, revision).length === 0
        );
      } catch {
        return false; // commit not present locally
      }
    });
  return {
    reusable: covering.find((run) => run.conclusion === 'success'),
    pending: covering.find((run) => run.status !== 'completed'),
  };
}

function resignCommand(runId) {
  const base = 'node scripts/resign-gemma-candidate.mjs --approve-risk-accepted';
  return runId === undefined ? base : `${base} --qualification-run ${runId}`;
}

/** Remedy commands for a gate failure of `kind` in `mode`. */
export function remedies({ mode, kind, runs }) {
  if (kind === 'not-ancestor') {
    return ['merge or rebase so the release commit descends from the signed candidate'];
  }
  if (kind === 'variables') {
    return [
      'gh variable list   # SEMANTIC_RELEASE_QUALIFIED and the mode approval variable must be "true"',
    ];
  }
  if (kind !== 'worker-changed') {
    return ['fix the manifest named above, see docs/semantic-operations.md'];
  }
  if (mode !== 'risk-accepted') {
    return [
      `${mode} Gemma has no mechanical re-sign: it needs reviewed approval evidence, see docs/semantic-operations.md`,
    ];
  }
  if (runs?.reusable !== undefined) {
    return [
      `${resignCommand(runs.reusable.databaseId)}   # qualification run ${runs.reusable.databaseId} already covers HEAD's worker inputs; no new run needed`,
    ];
  }
  if (runs?.pending !== undefined) {
    return [
      `wait for qualification run ${runs.pending.databaseId} (it covers HEAD's worker inputs), then ${resignCommand(runs.pending.databaseId)}`,
    ];
  }
  return [
    `${resignCommand()}   # qualifies on all four targets, commits the manifest, publishes the tag`,
  ];
}

function gate(mode, manifest, vars, revision, root) {
  const qualified = vars?.SEMANTIC_RELEASE_QUALIFIED;
  if (mode === 'experimental') {
    return checkDesktopGemmaRelease(
      manifest,
      qualified,
      vars?.SEMANTIC_GEMMA_EXPERIMENTAL_APPROVED,
      revision,
      root,
    );
  }
  if (mode === 'qualified') {
    return checkQualifiedGemmaDesktopRelease(
      manifest,
      qualified,
      vars?.SEMANTIC_GEMMA_NON_EXPERIMENTAL_APPROVED,
      revision,
      root,
    );
  }
  return checkRiskAcceptedGemmaDesktopRelease(
    manifest,
    qualified,
    vars?.SEMANTIC_GEMMA_RISK_ACCEPTED,
    desktopTarget,
    revision,
    root,
  );
}

/**
 * Evaluates every gate. `env` supplies everything with side effects so tests can fake it:
 * `{ root, tag, revision, vars, mode, existingTags, onOriginMain, dirty, readManifest,
 *    qualificationRuns, isAncestor, publishedTags }`. `vars` is undefined when GitHub could not
 * be read; then no variable gate is enforced and the active mode must be given or is inferred
 * from the manifests present.
 */
export function runPreflight(env) {
  const checks = [];
  const add = (check) => checks.push(check);
  const version = workspaceVersion(fs.readFileSync(`${env.root}/Cargo.toml`, 'utf8'));

  add(
    env.tag === `v${version}`
      ? { id: 'tag-version', ok: true, title: `tag ${env.tag} matches workspace version` }
      : {
          id: 'tag-version',
          ok: false,
          title: `tag ${env.tag} does not match workspace version v${version}`,
          fix: [`bump [workspace.package].version, or use the tag v${version}`],
        },
  );
  add(
    env.existingTags.includes(env.tag)
      ? {
          id: 'tag-free',
          ok: false,
          title: `tag ${env.tag} already exists (release tags are immutable)`,
          fix: ['bump [workspace.package].version to a new number'],
        }
      : { id: 'tag-free', ok: true, title: `tag ${env.tag} is unused` },
  );
  add(
    env.onOriginMain
      ? { id: 'on-main', ok: true, title: 'release commit is on origin/main' }
      : {
          id: 'on-main',
          ok: false,
          title: 'release commit is not on origin/main (the workflow rejects it)',
          fix: ['push the commit to main first, and wait for CI'],
        },
  );
  if (env.dirty) {
    add({
      id: 'clean-tree',
      ok: true,
      level: 'warn',
      title: 'working tree has uncommitted changes (the tag only covers committed work)',
    });
  }

  let modes;
  let inferred = false;
  if (env.mode !== undefined) {
    modes = [env.mode];
  } else if (env.vars !== undefined) {
    modes = activeGemmaModes(env.vars);
  } else {
    modes = Object.keys(gemmaModes).filter((mode) => env.readManifest(mode) !== undefined);
    inferred = true;
  }
  if (env.vars === undefined) {
    add({
      id: 'variables',
      ok: true,
      level: 'warn',
      title: 'GitHub variables unreadable (gh missing or offline): variable gates not checked',
      detail: inferred
        ? 'active Gemma mode inferred from manifests; pass --mode to pick one'
        : undefined,
    });
  } else if (modes.length > 1) {
    add({
      id: 'modes',
      ok: false,
      title: `Gemma modes combined: ${modes.join(', ')} (the workflow refuses this)`,
      fix: ['unset all but one of the SEMANTIC_GEMMA_* mode variables'],
    });
  } else if (env.vars?.SEMANTIC_RELEASE_QUALIFIED !== 'true' && modes.length > 0) {
    add({
      id: 'variables',
      ok: false,
      title: 'SEMANTIC_RELEASE_QUALIFIED is not "true"',
      fix: ['gh variable set SEMANTIC_RELEASE_QUALIFIED --body true'],
    });
  }
  if (modes.length === 0) {
    add({
      id: 'gemma',
      ok: true,
      level: 'warn',
      title: 'no Gemma mode active: the release ships without the Gemma gate',
    });
  }

  for (const mode of modes) {
    const manifest = env.readManifest(mode);
    if (manifest === undefined) {
      add({
        id: `gemma-${mode}`,
        ok: false,
        title: `${mode} Gemma manifest is missing`,
        fix: [`record the ${mode} approval, see docs/semantic-operations.md`],
      });
      continue;
    }
    // When variables are unreadable, assume the approvals are set so only source gates count.
    const vars = env.vars ?? {
      SEMANTIC_RELEASE_QUALIFIED: 'true',
      SEMANTIC_GEMMA_EXPERIMENTAL_APPROVED: 'true',
      SEMANTIC_GEMMA_NON_EXPERIMENTAL_APPROVED: 'true',
      SEMANTIC_GEMMA_RISK_ACCEPTED: 'true',
    };
    const candidate = `${mode} Gemma candidate ${manifest.releaseTag} (${manifest.sourceRevision.slice(0, 8)})`;
    try {
      gate(mode, manifest, vars, env.revision, env.root);
      add({
        id: `gemma-${mode}`,
        ok: true,
        title: `${candidate} is current for ${env.revision.slice(0, 8)}`,
      });
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      const kind = classifyGateFailure(message);
      const runs =
        kind === 'worker-changed' && env.qualificationRuns !== undefined
          ? classifyQualificationRuns(env.qualificationRuns, env.revision, {
              isAncestor: env.isAncestor,
              changesBetween: (from, to) => semanticWorkerChanges(from, to, env.root),
            })
          : undefined;
      add({
        id: `gemma-${mode}`,
        ok: false,
        title: `${candidate} fails the desktop gate`,
        detail: message,
        fix: remedies({ mode, kind, runs }),
      });
    }
    if (env.publishedTags !== undefined && !env.publishedTags.includes(manifest.releaseTag)) {
      add({
        id: `published-${mode}`,
        ok: true,
        level: 'warn',
        title: `signed candidate ${manifest.releaseTag} is not a published GitHub release`,
        detail: 'desktop builds fetch the signed catalog from it',
      });
    }
  }

  return { ok: checks.every((check) => check.ok), checks };
}

export function formatReport({ ok, checks }) {
  const lines = [];
  for (const check of checks) {
    const label = !check.ok ? 'FAIL' : check.level === 'warn' ? 'WARN' : 'PASS';
    lines.push(`${label}  ${check.title}`);
    for (const detail of check.detail?.split('\n') ?? []) lines.push(`        ${detail}`);
    for (const fix of check.fix ?? []) lines.push(`      fix: ${fix}`);
  }
  lines.push(
    ok
      ? 'Preflight passed: the desktop release gates would pass.'
      : 'Preflight failed: do not tag yet.',
  );
  return lines.join('\n');
}

function git(args, options = {}) {
  return execFileSync('git', args, {
    cwd: repoRoot,
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'ignore'],
    ...options,
  }).trim();
}

function tryGh(args) {
  try {
    return JSON.parse(
      execFileSync('gh', args, {
        cwd: repoRoot,
        encoding: 'utf8',
        stdio: ['ignore', 'pipe', 'ignore'],
      }),
    );
  } catch {
    return undefined;
  }
}

function parseArguments(argv) {
  const options = { fetch: true, gh: true };
  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    if (argument === '--tag') options.tag = argv[++index];
    else if (argument === '--revision') options.revision = argv[++index];
    else if (argument === '--mode') options.mode = argv[++index];
    else if (argument === '--no-fetch') options.fetch = false;
    else if (argument === '--no-gh') options.gh = false;
    else throw new Error(`unknown argument: ${argument}`);
  }
  if (options.mode !== undefined && !(options.mode in gemmaModes)) {
    throw new Error(`--mode must be one of ${Object.keys(gemmaModes).join(', ')}`);
  }
  return options;
}

function main() {
  const options = parseArguments(process.argv.slice(2));
  if (options.fetch) {
    try {
      git(['fetch', '--quiet', '--tags', 'origin', 'main']);
    } catch {
      process.stderr.write('warning: could not fetch origin; using local refs\n');
    }
  }
  const revision = git(['rev-parse', options.revision ?? 'HEAD']);
  const isAncestor = (ancestor, descendant) => {
    try {
      git(['merge-base', '--is-ancestor', ancestor, descendant]);
      return true;
    } catch {
      return false;
    }
  };
  const variables = options.gh ? tryGh(['variable', 'list', '--json', 'name,value']) : undefined;
  const report = runPreflight({
    root: repoRoot,
    tag: options.tag ?? `v${workspaceVersion(fs.readFileSync(`${repoRoot}/Cargo.toml`, 'utf8'))}`,
    revision,
    vars:
      variables === undefined
        ? undefined
        : Object.fromEntries(variables.map(({ name, value }) => [name, value])),
    mode: options.mode,
    existingTags: git(['tag', '--list']).split('\n'),
    onOriginMain: isAncestor(revision, 'origin/main'),
    dirty: git(['status', '--porcelain']) !== '',
    readManifest: (mode) =>
      fs.existsSync(gemmaModes[mode].manifestPath)
        ? JSON.parse(fs.readFileSync(gemmaModes[mode].manifestPath, 'utf8'))
        : undefined,
    qualificationRuns: options.gh
      ? tryGh([
          'run',
          'list',
          '--workflow',
          qualificationWorkflow,
          '--branch',
          'main',
          '--limit',
          '30',
          '--json',
          'databaseId,headSha,createdAt,event,status,conclusion',
        ])
      : undefined,
    isAncestor,
    publishedTags: options.gh
      ? tryGh(['release', 'list', '--limit', '100', '--json', 'tagName'])?.map(
          ({ tagName }) => tagName,
        )
      : undefined,
  });
  console.log(formatReport(report));
  process.exitCode = report.ok ? 0 : 1;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  main();
}
