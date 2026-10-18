import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import { fileURLToPath } from 'node:url';

import { semanticComponentReleasePlan } from './fetch-approved-semantic-catalog.mjs';

const repoRoot = fileURLToPath(new URL('..', import.meta.url));
const semanticWorkerBuildPaths = [
  'Cargo.lock',
  'Cargo.toml',
  'rust-toolchain.toml',
  'crates/fm-metadata',
  'crates/fm-semantic-components',
  'crates/fm-semantic-conversion',
  'crates/fm-semantic-docling',
  'crates/fm-semantic-protocol',
  'crates/fm-semantic-worker',
];

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

  const changedPaths = execFileSync(
    'git',
    [
      'diff',
      '--name-only',
      manifestSourceRevision,
      releaseSourceRevision,
      '--',
      ...semanticWorkerBuildPaths,
    ],
    { cwd, encoding: 'utf8' },
  ).trim();
  if (changedPaths.length > 0) {
    throw new Error(
      `semantic worker build inputs changed after signed candidate:\n${changedPaths}`,
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
