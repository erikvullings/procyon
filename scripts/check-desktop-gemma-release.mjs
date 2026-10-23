import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import { fileURLToPath } from 'node:url';

import { semanticComponentReleasePlan } from './fetch-approved-semantic-catalog.mjs';

const repoRoot = fileURLToPath(new URL('..', import.meta.url));
const semanticWorkerCrate = 'fm-semantic-worker';
// Directories inside a crate that never contribute to its compiled library or binary.
const nonBuildCrateDirectories = ['tests', 'benches', 'examples'];

function git(cwd, args) {
  return execFileSync('git', args, { cwd, encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] });
}

function gitShow(cwd, revision, file) {
  try {
    return git(cwd, ['show', `${revision}:${file}`]);
  } catch {
    return undefined;
  }
}

/** Parses the `[[package]]` tables of a `Cargo.lock`. */
export function parseCargoLock(text) {
  return text
    .split('[[package]]')
    .slice(1)
    .map((block) => {
      const field = (name) => new RegExp(`^${name} = "([^"]*)"`, 'mu').exec(block)?.[1];
      const list = /^dependencies = \[([^\]]*)\]/mu.exec(block)?.[1] ?? '';
      return {
        name: field('name'),
        version: field('version'),
        source: field('source'),
        checksum: field('checksum'),
        dependencies: [...list.matchAll(/"([^"]+)"/gu)].map((match) => match[1]),
      };
    });
}

/**
 * Locked packages reachable from the worker, keyed by lock-file identity. Workspace members have
 * no `source`; their own version is a release number, not a worker build input, so it is left out
 * to keep version bumps from counting as worker changes.
 */
export function semanticWorkerLockClosure(lockText) {
  const packages = parseCargoLock(lockText);
  const workspaceNames = new Set(
    packages.filter((entry) => entry.source === undefined).map((entry) => entry.name),
  );
  const identity = (entry) =>
    workspaceNames.has(entry.name) ? entry.name : `${entry.name} ${entry.version}`;
  const resolve = (reference) => {
    const [name, version] = reference.split(' ');
    return packages.filter(
      (entry) => entry.name === name && (version === undefined || entry.version === version),
    );
  };
  const closure = new Map();
  const pending = packages.filter((entry) => entry.name === semanticWorkerCrate);
  while (pending.length > 0) {
    const entry = pending.pop();
    if (closure.has(identity(entry))) continue;
    closure.set(identity(entry), {
      source: entry.source,
      checksum: entry.checksum,
      dependencies: entry.dependencies
        .map((reference) => identity(resolve(reference)[0] ?? { name: reference }))
        .sort(),
    });
    pending.push(...entry.dependencies.flatMap(resolve));
  }
  return closure;
}

/**
 * The root manifest minus the lines that change without altering any worker build: the
 * workspace-wide release version and the member list.
 */
export function normalizedRootManifest(text) {
  return text
    .replace(/^(version\s*=\s*)"[^"]*"/mu, '$1"-"')
    .replace(/^members\s*=\s*\[[^\]]*\]/mu, 'members = []');
}

/** Everything that differs between two revisions in what builds the signed semantic worker. */
export function semanticWorkerChanges(manifestSourceRevision, releaseSourceRevision, cwd) {
  const lockBefore = gitShow(cwd, manifestSourceRevision, 'Cargo.lock') ?? '';
  const lockAfter = gitShow(cwd, releaseSourceRevision, 'Cargo.lock') ?? '';
  const closureBefore = semanticWorkerLockClosure(lockBefore);
  const closureAfter = semanticWorkerLockClosure(lockAfter);
  const changes = [...new Set([...closureBefore.keys(), ...closureAfter.keys()])]
    .filter(
      (key) =>
        JSON.stringify(closureBefore.get(key) ?? null) !==
        JSON.stringify(closureAfter.get(key) ?? null),
    )
    .map((key) => `Cargo.lock: ${key}`);

  const rootBefore = gitShow(cwd, manifestSourceRevision, 'Cargo.toml') ?? '';
  const rootAfter = gitShow(cwd, releaseSourceRevision, 'Cargo.toml') ?? '';
  if (normalizedRootManifest(rootBefore) !== normalizedRootManifest(rootAfter)) {
    changes.push('Cargo.toml');
  }

  // The worker plus every workspace crate it can pull in.
  const crates = new Set([semanticWorkerCrate]);
  for (const closure of [closureBefore, closureAfter]) {
    for (const [key, entry] of closure) {
      if (entry.source === undefined && fs.existsSync(`${cwd}/crates/${key}`)) crates.add(key);
    }
  }
  const pathspecs = [
    'rust-toolchain.toml',
    ...[...crates].flatMap((name) => [
      `crates/${name}`,
      ...nonBuildCrateDirectories.map((directory) => `:(exclude)crates/${name}/${directory}`),
    ]),
  ];
  const changedPaths = git(cwd, [
    'diff',
    '--name-only',
    manifestSourceRevision,
    releaseSourceRevision,
    '--',
    ...pathspecs,
  ])
    .split('\n')
    .filter((line) => line.length > 0);
  return [...changes, ...changedPaths];
}

export const experimentalManifestPath = fileURLToPath(
  new URL('../docs/evaluations/semantic-gemma-experimental-v1.json', import.meta.url),
);

export function verifySemanticWorkerSource(manifestSourceRevision, releaseSourceRevision, cwd) {
  try {
    execFileSync(
      'git',
      ['merge-base', '--is-ancestor', manifestSourceRevision, releaseSourceRevision],
      { cwd, stdio: 'ignore' },
    );
  } catch {
    throw new Error('signed Gemma candidate source is not an ancestor of the desktop release');
  }

  const changed = semanticWorkerChanges(manifestSourceRevision, releaseSourceRevision, cwd);
  if (changed.length > 0) {
    throw new Error(
      `semantic worker build inputs changed after signed candidate:\n${changed.join('\n')}`,
    );
  }
}

export function checkDesktopGemmaRelease(
  manifest,
  semanticEnabled,
  experimentalEnabled,
  releaseSourceRevision,
) {
  if (semanticEnabled !== 'true' || experimentalEnabled !== 'true') {
    throw new Error('experimental Gemma desktop requires both independent release approvals');
  }
  if (!manifest.experimentalGemma) {
    throw new Error('an explicit experimental Gemma approval is required');
  }
  if (releaseSourceRevision !== undefined) {
    verifySemanticWorkerSource(manifest.sourceRevision, releaseSourceRevision, repoRoot);
  }
  for (const target of ['linux-aarch64', 'linux-x86_64', 'macos-aarch64', 'windows-x86_64']) {
    semanticComponentReleasePlan(manifest, target);
  }
  return manifest;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const manifest = JSON.parse(fs.readFileSync(experimentalManifestPath, 'utf8'));
  const releaseSourceRevisionIndex = process.argv.indexOf('--release-source-revision');
  const releaseSourceRevision =
    releaseSourceRevisionIndex === -1 ? undefined : process.argv[releaseSourceRevisionIndex + 1];
  if (
    releaseSourceRevisionIndex !== -1 &&
    (!releaseSourceRevision || !/^[0-9a-f]{40}$/u.test(releaseSourceRevision))
  ) {
    throw new Error('desktop release source must be a 40-character commit');
  }
  checkDesktopGemmaRelease(
    manifest,
    process.env.SEMANTIC_RELEASE_QUALIFIED,
    process.env.SEMANTIC_GEMMA_EXPERIMENTAL_APPROVED,
    releaseSourceRevision,
  );
  console.log(
    `Experimental Gemma desktop uses reviewed signed candidate ${manifest.qualificationRunId}`,
  );
}
