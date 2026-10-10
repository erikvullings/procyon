import fs from 'node:fs';
import { fileURLToPath } from 'node:url';

import { semanticComponentReleasePlan } from './fetch-approved-semantic-catalog.mjs';

export const experimentalManifestPath = fileURLToPath(
  new URL('../docs/evaluations/semantic-gemma-experimental-v1.json', import.meta.url),
);

export function checkDesktopGemmaRelease(manifest, semanticEnabled, experimentalEnabled) {
  if (semanticEnabled !== 'true' || experimentalEnabled !== 'true') {
    throw new Error('experimental Gemma desktop requires both independent release approvals');
  }
  if (!manifest.experimentalGemma) {
    throw new Error('an explicit experimental Gemma approval is required');
  }
  for (const target of ['linux-aarch64', 'linux-x86_64', 'macos-aarch64', 'windows-x86_64']) {
    semanticComponentReleasePlan(manifest, target);
  }
  return manifest;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const manifest = JSON.parse(fs.readFileSync(experimentalManifestPath, 'utf8'));
  checkDesktopGemmaRelease(
    manifest,
    process.env.SEMANTIC_RELEASE_QUALIFIED,
    process.env.SEMANTIC_GEMMA_EXPERIMENTAL_APPROVED,
  );
  console.log(
    `Experimental Gemma desktop uses reviewed signed candidate ${manifest.qualificationRunId}`,
  );
}
