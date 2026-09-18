import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';

import { createSemanticComponentReleaseManifest } from './create-semantic-component-release-manifest.mjs';
import {
  semanticComponentReleasePlan,
  verifySemanticCatalogBytes,
} from './fetch-approved-semantic-catalog.mjs';
import { verifySemanticPayloads } from './verify-semantic-component-release.mjs';

function sha256(value) {
  return createHash('sha256').update(value).digest('hex');
}

const payload = Buffer.from('worker-bytes');
const catalog = Buffer.from(
  `${JSON.stringify({
    catalog: {
      revision: 'catalog-linux',
      artifacts: [
        {
          id: 'worker.abc',
          location:
            'https://github.com/erikvullings/procyon/releases/download/semantic-v1/worker.abc',
          checksum: [...Buffer.from(sha256(payload), 'hex')],
          resources: { download_bytes: payload.byteLength },
        },
      ],
      production: { source_revision: 'abc123' },
    },
  })}\n`,
);
const signature = Buffer.from('detached-signature');
const manifest = {
  schemaVersion: 1,
  decision: 'go',
  repository: 'erikvullings/procyon',
  releaseTag: 'semantic-v1',
  qualificationRunId: 123,
  sourceRevision: 'abc123',
  releaseCandidateFingerprint: 'sha256:fingerprint',
  targets: {
    'linux-x86_64': {
      catalogRevision: 'catalog-linux',
      catalogSha256: sha256(catalog),
      signatureSha256: sha256(signature),
    },
  },
};

test('approved component plans use immutable release asset URLs', () => {
  const plan = semanticComponentReleasePlan(manifest, 'linux-x86_64');
  assert.equal(
    plan.catalogUrl,
    'https://github.com/erikvullings/procyon/releases/download/semantic-v1/semantic-catalog-linux-x86_64.json',
  );
  assert.equal(
    plan.signatureUrl,
    'https://github.com/erikvullings/procyon/releases/download/semantic-v1/semantic-catalog-linux-x86_64.sig',
  );
});

test('catalog bytes must match the reviewed hashes, revision, source, and release URLs', () => {
  const plan = semanticComponentReleasePlan(manifest, 'linux-x86_64');
  assert.doesNotThrow(() => verifySemanticCatalogBytes(plan, catalog, signature));
  assert.throws(
    () => verifySemanticCatalogBytes(plan, Buffer.from('changed'), signature),
    /catalog SHA-256/,
  );
  const wrongSource = Buffer.from(
    catalog.toString().replace('"source_revision":"abc123"', '"source_revision":"other"'),
  );
  manifest.targets['linux-x86_64'].catalogSha256 = sha256(wrongSource);
  const wrongPlan = semanticComponentReleasePlan(manifest, 'linux-x86_64');
  assert.throws(
    () => verifySemanticCatalogBytes(wrongPlan, wrongSource, signature),
    /source revision/,
  );
  const wrongLocation = Buffer.from(
    catalog
      .toString()
      .replace('/worker.abc"', '/different.abc"')
      .replace('"source_revision":"abc123"', '"source_revision":"other"'),
  );
  manifest.targets['linux-x86_64'].catalogSha256 = sha256(wrongLocation);
  const wrongLocationPlan = {
    ...semanticComponentReleasePlan(manifest, 'linux-x86_64'),
    sourceRevision: 'other',
  };
  assert.throws(
    () => verifySemanticCatalogBytes(wrongLocationPlan, wrongLocation, signature),
    /approved immutable release/,
  );
});

test('unapproved or incomplete component releases fail closed', () => {
  assert.throws(
    () => semanticComponentReleasePlan({ ...manifest, decision: 'no-go' }, 'linux-x86_64'),
    /approved semantic component release/,
  );
  assert.throws(
    () => semanticComponentReleasePlan(manifest, 'windows-x86_64'),
    /target windows-x86_64/,
  );
});

test('publication verifies every payload against its catalog fingerprint', (context) => {
  const root = mkdtempSync(join(tmpdir(), 'semantic-component-payload-'));
  context.after(() => rmSync(root, { force: true, recursive: true }));
  writeFileSync(join(root, 'worker.abc'), payload);
  const envelope = JSON.parse(catalog.toString());
  assert.doesNotThrow(() => verifySemanticPayloads(envelope, root, 'linux-x86_64'));
  writeFileSync(join(root, 'worker.abc'), 'changed');
  assert.throws(
    () => verifySemanticPayloads(envelope, root, 'linux-x86_64'),
    /payload worker\.abc SHA-256/,
  );
  envelope.catalog.artifacts[0].id = '../worker.abc';
  assert.throws(() => verifySemanticPayloads(envelope, root, 'linux-x86_64'), /invalid payload ID/);
});

test('qualification emits a reviewable lock over every catalog and signature', (context) => {
  const root = mkdtempSync(join(tmpdir(), 'semantic-component-release-'));
  context.after(() => rmSync(root, { force: true, recursive: true }));
  for (const target of ['linux-aarch64', 'linux-x86_64', 'macos-aarch64', 'windows-x86_64']) {
    const targetCatalog = Buffer.from(
      catalog
        .toString()
        .replace('"catalog-linux"', `"catalog-${target}"`)
        .replace('"worker.abc"', `"worker.${target}"`)
        .replace('/worker.abc"', `/worker.${target}"`),
    );
    writeFileSync(join(root, `semantic-catalog-${target}.json`), targetCatalog);
    writeFileSync(join(root, `semantic-catalog-${target}.sig`), signature);
  }
  const generated = createSemanticComponentReleaseManifest({
    assetsRoot: root,
    evaluation: { decision: 'go', releaseCandidateFingerprint: 'sha256:fingerprint' },
    repository: 'erikvullings/procyon',
    releaseTag: 'semantic-v1',
    qualificationRunId: 123,
    sourceRevision: 'abc123',
  });
  assert.equal(generated.decision, 'go');
  assert.equal(Object.keys(generated.targets).length, 4);
  assert.equal(generated.targets['macos-aarch64'].catalogRevision, 'catalog-macos-aarch64');
  assert.equal(generated.targets['windows-x86_64'].signatureSha256, sha256(signature));
});
