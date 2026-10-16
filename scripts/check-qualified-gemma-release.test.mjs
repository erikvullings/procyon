import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import {
  checkQualifiedGemmaDesktopRelease,
  checkQualifiedGemmaRelease,
} from './check-qualified-gemma-release.mjs';
import { createSemanticComponentReleaseManifest } from './create-semantic-component-release-manifest.mjs';

const targets = ['linux-aarch64', 'linux-x86_64', 'macos-aarch64', 'windows-x86_64'];
const modelRevision = '9'.repeat(40);
const gateNames = [
  'resourceCeilings',
  'cpuFallback',
  'crossHostNavigation',
  'upgradeRollback',
  'mediaSearch',
];
const gemmaFiles = [
  ['embeddinggemma2-original', 'model.safetensors'],
  ['embeddinggemma2-file-1', 'tokenizer.json'],
  ['embeddinggemma2-file-2', 'config.json'],
  ['embeddinggemma2-file-3', 'processor_config.json'],
  ['embeddinggemma2-file-4', 'preprocessor_config.json'],
];

function sha256(bytes) {
  return createHash('sha256').update(bytes).digest('hex');
}

function fixture(context) {
  const root = mkdtempSync(join(tmpdir(), 'gemma-qualified-release-'));
  context.after(() => rmSync(root, { recursive: true, force: true }));
  const assets = join(root, 'assets');
  const candidateRoot = join(root, 'candidate');
  mkdirSync(assets);
  mkdirSync(candidateRoot);
  const sourceRevision = 'a'.repeat(40);
  const artifacts = [
    ...gemmaFiles.map(([component, name]) => ({ component, id: `${component}.bin`, name })),
    { component: 'procyon.semantic.worker', id: 'worker.bin', name: 'worker' },
  ];
  for (const artifact of artifacts) {
    writeFileSync(join(assets, artifact.id), artifact.name);
  }
  for (const target of targets) {
    const catalog = {
      catalog: {
        revision: `catalog-${target}`,
        models: [
          {
            artifact_id: 'embeddinggemma2-original.bin',
            primary_file_name: 'model.safetensors',
            files: Object.fromEntries(
              gemmaFiles.slice(1).map(([component, name]) => [name, `${component}.bin`]),
            ),
            metadata: { identity: { model: 'google-embeddinggemma-2', revision: modelRevision } },
          },
        ],
        artifacts: artifacts.map((artifact) => ({
          id: artifact.id,
          component_id: artifact.component,
          location: `https://github.com/erikvullings/procyon/releases/download/semantic-v2/${artifact.id}`,
          checksum: [...Buffer.from(sha256(artifact.name), 'hex')],
          resources: { download_bytes: Buffer.byteLength(artifact.name) },
        })),
      },
      provenance: [
        {
          artifact_id: 'worker.bin',
          source: 'https://github.com/erikvullings/procyon',
          source_revision: sourceRevision,
        },
      ],
    };
    writeFileSync(join(assets, `semantic-catalog-${target}.json`), `${JSON.stringify(catalog)}\n`);
    writeFileSync(join(assets, `semantic-catalog-${target}.sig`), `signature-${target}`);
  }
  const candidate = createSemanticComponentReleaseManifest({
    assetsRoot: assets,
    evaluation: { decision: 'noGo', releaseCandidateFingerprint: 'sha256:eval' },
    repository: 'erikvullings/procyon',
    releaseTag: 'semantic-v2',
    qualificationRunId: 42,
    sourceRevision,
  });
  const candidateBytes = Buffer.from(`${JSON.stringify(candidate, null, 2)}\n`);
  writeFileSync(join(candidateRoot, 'semantic-component-release.json'), candidateBytes);
  const approval = {
    ...candidate,
    decision: 'go',
    nonExperimentalGemma: {
      approval: 'release-owner-approved-five-gate-qualification',
      candidateDecision: candidate.decision,
      modelRevision,
      candidateManifestSha256: sha256(candidateBytes),
      approvedBy: 'release-owner',
      approvedAt: '2026-10-16T12:00:00Z',
      gates: Object.fromEntries(gateNames.map((gate) => [gate, { result: 'Pass' }])),
    },
  };
  return { assets, candidateRoot, candidate, candidateBytes, approval, sourceRevision };
}

test('five passing gates approve only the exact new signed candidate', (context) => {
  const { assets, candidateRoot, approval, sourceRevision } = fixture(context);
  assert.deepEqual(checkQualifiedGemmaRelease(assets, candidateRoot, approval, 42, 'semantic-v2'), {
    releaseTag: 'semantic-v2',
    sourceRevision,
  });
});

test('every five-gate result is mandatory and must be recorded as Pass', (context) => {
  const { assets, candidateRoot, approval } = fixture(context);
  for (const gate of gateNames) {
    const changed = {
      ...approval,
      nonExperimentalGemma: {
        ...approval.nonExperimentalGemma,
        gates: {
          ...approval.nonExperimentalGemma.gates,
          [gate]: { result: 'Blocked' },
        },
      },
    };
    assert.throws(
      () => checkQualifiedGemmaRelease(assets, candidateRoot, changed, 42, 'semantic-v2'),
      /five-gate approval/u,
    );
  }
});

test('approval is bound to candidate bytes, source, run, tag, and signed catalog contents', (context) => {
  const { assets, candidateRoot, candidate, candidateBytes, approval } = fixture(context);
  const changedDigest = {
    ...approval,
    nonExperimentalGemma: {
      ...approval.nonExperimentalGemma,
      candidateManifestSha256: '0'.repeat(64),
    },
  };
  assert.throws(
    () => checkQualifiedGemmaRelease(assets, candidateRoot, changedDigest, 42, 'semantic-v2'),
    /exact new signed candidate/u,
  );
  assert.throws(
    () => checkQualifiedGemmaRelease(assets, candidateRoot, approval, 43, 'semantic-v2'),
    /exact new signed candidate/u,
  );
  assert.throws(
    () => checkQualifiedGemmaRelease(assets, candidateRoot, approval, 42, 'semantic-v3'),
    /exact new signed candidate/u,
  );
  const oldCandidate = { ...candidate, sourceRevision: '4553da275e76638678c7fc0a6ddcea732a19165b' };
  writeFileSync(
    join(candidateRoot, 'semantic-component-release.json'),
    `${JSON.stringify(oldCandidate, null, 2)}\n`,
  );
  assert.throws(
    () => checkQualifiedGemmaRelease(assets, candidateRoot, approval, 42, 'semantic-v2'),
    /exact new signed candidate/u,
  );
  writeFileSync(join(candidateRoot, 'semantic-component-release.json'), candidateBytes);
  const targetCatalog = join(assets, 'semantic-catalog-macos-aarch64.json');
  const catalog = JSON.parse(readFileSync(targetCatalog));
  catalog.catalog.models[0].metadata.identity.revision = '0'.repeat(40);
  writeFileSync(targetCatalog, `${JSON.stringify(catalog)}\n`);
  assert.throws(
    () => checkQualifiedGemmaRelease(assets, candidateRoot, approval, 42, 'semantic-v2'),
    /catalog SHA-256/u,
  );
});

test('desktop promotion requires the independent approval gates and arm64-only macOS', (context) => {
  const { approval } = fixture(context);
  assert.throws(
    () => checkQualifiedGemmaDesktopRelease(approval, 'false', 'true', 'arm64'),
    /both independent release approvals/u,
  );
  assert.throws(
    () => checkQualifiedGemmaDesktopRelease(approval, 'true', 'false', 'arm64'),
    /both independent release approvals/u,
  );
  assert.deepEqual(checkQualifiedGemmaDesktopRelease(approval, 'true', 'true', 'arm64'), approval);
  assert.throws(
    () => checkQualifiedGemmaDesktopRelease(approval, 'true', 'true', 'universal'),
    /arm64-only/u,
  );
  const blocked = {
    ...approval,
    nonExperimentalGemma: {
      ...approval.nonExperimentalGemma,
      gates: {
        ...approval.nonExperimentalGemma.gates,
        resourceCeilings: { result: 'Fail' },
      },
    },
  };
  assert.throws(
    () => checkQualifiedGemmaDesktopRelease(blocked, 'true', 'true', 'arm64'),
    /five-gate approval/u,
  );
});
