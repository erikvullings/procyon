import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';

import { gemmaInstalledLimitations } from './gemma-installed-limitations.mjs';

const dimensions = (backend) =>
  [128, 256, 512, 768].map((size) => ({
    dimensions: size,
    workerComputeBackend: backend,
  }));

test('installed Metal report describes partial Metal compute without claiming a paired speedup', () => {
  const limitations = gemmaInstalledLimitations(dimensions('google/embeddinggemma-2 (FP32 Metal)'));
  assert.match(limitations, /FP32 Metal selected/u);
  assert.match(limitations, /audio preprocessing and video decoding still run on CPU/u);
  assert.match(limitations, /no quality labels, paired CPU\/Metal comparison/u);
  assert.doesNotMatch(limitations, /CPU runner class|no GPU measurements/u);
});

test('installed CPU report does not claim Metal and mixed backend records fail closed', () => {
  assert.match(
    gemmaInstalledLimitations(dimensions('google/embeddinggemma-2 (native CPU)')),
    /CPU selected for the worker; no Metal measurement/u,
  );
  assert.throws(
    () =>
      gemmaInstalledLimitations([
        ...dimensions('google/embeddinggemma-2 (native CPU)').slice(0, 3),
        ...dimensions('google/embeddinggemma-2 (FP32 Metal)').slice(3),
      ]),
    /one reported backend/u,
  );
});

test('read-only installed workflow uses the tested report limitations generator', () => {
  const workflow = readFileSync('.github/workflows/release-semantic-components.yml', 'utf8');
  assert.match(workflow, /gemmaInstalledLimitations\(dimensions\)/u);
});
