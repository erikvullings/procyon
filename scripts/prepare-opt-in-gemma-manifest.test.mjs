import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import fs from 'node:fs';
import { test } from 'node:test';

import { optInGemmaManifestPath } from './gemma-opt-in.mjs';
import { prepareOptInGemmaManifest } from './prepare-opt-in-gemma-manifest.mjs';

const current = JSON.parse(fs.readFileSync(optInGemmaManifestPath, 'utf8'));
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
  modelRevision: current.optInGemma.modelRevision,
  approvedBy: 'erikvullings',
  approvedAt: '2026-10-25T12:00:00Z',
};

test('binds the manifest to the exact candidate lock bytes', () => {
  const manifest = prepareOptInGemmaManifest(candidate, owner);
  assert.equal(manifest.sourceRevision, 'a'.repeat(40));
  assert.equal(manifest.qualificationRunId, 42);
  assert.equal(
    manifest.optInGemma.candidateManifestSha256,
    createHash('sha256').update(candidate).digest('hex'),
  );
  assert.equal(manifest.optInGemma.candidateDecision, 'noGo');
});

test('rejects an invalid owner approval', () => {
  assert.throws(() => prepareOptInGemmaManifest(candidate, { ...owner, approvedAt: 'now' }));
});
