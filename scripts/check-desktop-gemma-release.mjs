import fs from 'node:fs';
import { fileURLToPath } from 'node:url';

import { semanticComponentReleasePlan } from './fetch-approved-semantic-catalog.mjs';

export const experimentalManifestPath = fileURLToPath(
  new URL('../docs/evaluations/semantic-gemma-experimental-v1.json', import.meta.url),
);

export function checkDesktopGemmaRelease(
  manifest,
  semanticEnabled,
  experimentalEnabled,
  macosArchitecture = 'universal',
  expectedSourceRevision,
) {
  if (semanticEnabled !== 'true' || experimentalEnabled !== 'true') {
    throw new Error('experimental Gemma desktop requires both independent release approvals');
  }
  if (!manifest.experimentalGemma) {
    throw new Error('an explicit experimental Gemma approval is required');
  }
  if (expectedSourceRevision !== undefined && manifest.sourceRevision !== expectedSourceRevision) {
    throw new Error('experimental Gemma worker source revision does not match desktop release');
  }
  for (const target of ['linux-aarch64', 'linux-x86_64', 'macos-aarch64', 'windows-x86_64']) {
    semanticComponentReleasePlan(manifest, target);
  }
  // A universal or Intel app cannot use the signed arm64-only Mac worker.
  if (macosArchitecture !== 'arm64') {
    throw new Error(
      'experimental universal macOS desktop requires an approved macos-x86_64 worker/catalog; arm64 alone cannot run on Intel',
    );
  }
  return manifest;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const manifest = JSON.parse(fs.readFileSync(experimentalManifestPath, 'utf8'));
  const sourceRevisionIndex = process.argv.indexOf('--source-revision');
  const expectedSourceRevision =
    sourceRevisionIndex === -1 ? undefined : process.argv[sourceRevisionIndex + 1];
  if (!expectedSourceRevision || !/^[0-9a-f]{40}$/u.test(expectedSourceRevision)) {
    throw new Error('experimental Gemma desktop requires --source-revision <40-character commit>');
  }
  checkDesktopGemmaRelease(
    manifest,
    process.env.SEMANTIC_RELEASE_QUALIFIED,
    process.env.SEMANTIC_GEMMA_EXPERIMENTAL_APPROVED,
    process.argv.includes('--macos-arm64-only') ? 'arm64' : 'universal',
    expectedSourceRevision,
  );
  console.log(
    `Experimental Gemma desktop uses reviewed signed candidate ${manifest.qualificationRunId}`,
  );
}
