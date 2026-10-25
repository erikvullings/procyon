import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import fs from 'node:fs';
import { test } from 'node:test';

import { riskAcceptedGemmaManifestPath } from './gemma-risk-acceptance.mjs';
import { prepareRiskAcceptedGemmaManifest } from './prepare-risk-accepted-gemma-manifest.mjs';

const current = JSON.parse(fs.readFileSync(riskAcceptedGemmaManifestPath, 'utf8'));
const candidate = Buffer.from(
  `${JSON.stringify({
    schemaVersion: 1,
    decision: 'noGo',
    repository: current.repository,
    releaseTag: 'semantic-v9',
    qualificationRunId: 42,
    sourceRevision: 'a'.repeat(40),
    releaseCandidateFingerprint: 'sha256:abc',
    targets: current.targets,
  })}\n`,
);
const owner = {
  modelRevision: current.riskAcceptedGemma.modelRevision,
  approvedBy: 'erikvullings',
  approvedAt: '2026-10-25T12:00:00Z',
};

test('binds the manifest to the exact candidate lock bytes', () => {
  const manifest = prepareRiskAcceptedGemmaManifest(candidate, owner);
  assert.equal(manifest.sourceRevision, 'a'.repeat(40));
  assert.equal(manifest.qualificationRunId, 42);
  assert.equal(
    manifest.riskAcceptedGemma.candidateManifestSha256,
    createHash('sha256').update(candidate).digest('hex'),
  );
  assert.equal(manifest.riskAcceptedGemma.candidateDecision, 'noGo');
});

test('rejects an invalid owner approval', () => {
  assert.throws(() => prepareRiskAcceptedGemmaManifest(candidate, { ...owner, approvedAt: 'now' }));
});
