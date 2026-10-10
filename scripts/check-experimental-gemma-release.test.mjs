import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

import { checkExperimentalGemmaRelease } from './check-experimental-gemma-release.mjs';
import { createSemanticComponentReleaseManifest } from './create-semantic-component-release-manifest.mjs';

const targets = ['linux-aarch64', 'linux-x86_64', 'macos-aarch64', 'windows-x86_64'];
const files = [
  ['embeddinggemma2-original', 'model.safetensors'],
  ['embeddinggemma2-file-1', 'tokenizer.json'],
  ['embeddinggemma2-file-2', 'config.json'],
  ['embeddinggemma2-file-3', 'processor_config.json'],
  ['embeddinggemma2-file-4', 'preprocessor_config.json'],
];

function sha(bytes) {
  return createHash('sha256').update(bytes).digest('hex');
}

function fixture(context) {
  const root = mkdtempSync(join(tmpdir(), 'gemma-experimental-release-'));
  context.after(() => rmSync(root, { recursive: true, force: true }));
  const assets = join(root, 'assets');
  const candidateRoot = join(root, 'candidate');
  mkdirSync(assets);
  mkdirSync(candidateRoot);
  const artifacts = [
    ...files.map(([component, name]) => ({ component, id: `${component}.bin`, content: name })),
    { component: 'procyon.semantic.worker', id: 'worker.bin', content: 'worker' },
  ];
  for (const item of artifacts) writeFileSync(join(assets, item.id), item.content);
  for (const target of targets) {
    const catalog = {
      catalog: {
        revision: `catalog-${target}`,
        models: [
          {
            artifact_id: 'embeddinggemma2-original.bin',
            primary_file_name: 'model.safetensors',
            files: Object.fromEntries(
              files.slice(1).map(([component, name]) => [name, `${component}.bin`]),
            ),
            metadata: {
              identity: {
                model: 'google-embeddinggemma-2',
                revision: '914f7f89142e33e77833254d9c9b90c3cef7303b',
              },
            },
          },
        ],
        artifacts: artifacts.map((item) => ({
          id: item.id,
          component_id: item.component,
          location: `https://github.com/o/r/releases/download/semantic-v1/${item.id}`,
          checksum: [...Buffer.from(sha(item.content), 'hex')],
          resources: { download_bytes: Buffer.byteLength(item.content) },
        })),
      },
      provenance: [
        {
          artifact_id: 'worker.bin',
          source: 'https://github.com/o/r',
          source_revision: 'source',
        },
      ],
    };
    writeFileSync(join(assets, `semantic-catalog-${target}.json`), `${JSON.stringify(catalog)}\n`);
    writeFileSync(join(assets, `semantic-catalog-${target}.sig`), 'signature');
  }
  const report = {
    evidencePolicy: 'experimental-alpha',
    productionMeasurement: true,
    decision: 'noGo',
    releaseCandidateFingerprint: 'sha256:candidate',
    measurements: targets.map((target) => ({
      identity: {
        target,
        pipeline: { model: { model: 'intfloat.multilingual-e5-small' } },
      },
    })),
  };
  writeFileSync(join(assets, 'semantic-production-evaluation.json'), `${JSON.stringify(report)}\n`);
  const candidate = createSemanticComponentReleaseManifest({
    assetsRoot: assets,
    evaluation: {
      decision: 'noGo',
      releaseCandidateFingerprint: report.releaseCandidateFingerprint,
    },
    repository: 'o/r',
    releaseTag: 'semantic-v1',
    qualificationRunId: 123,
    sourceRevision: 'source',
  });
  const candidateBytes = `${JSON.stringify(candidate, null, 2)}\n`;
  writeFileSync(join(candidateRoot, 'semantic-component-release.json'), candidateBytes);
  const approval = {
    ...candidate,
    decision: 'go',
    experimentalGemma: {
      approval: 'explicit-opt-in-experimental',
      candidateDecision: 'noGo',
      candidateManifestSha256: sha(candidateBytes),
      evaluationSha256: sha(readFileSync(join(assets, 'semantic-production-evaluation.json'))),
      modelRevision: '914f7f89142e33e77833254d9c9b90c3cef7303b',
    },
  };
  return { assets, candidateRoot, approval };
}

test('only the exact four-target signed candidate and E5 negative-control report pass', (context) => {
  const { assets, candidateRoot, approval } = fixture(context);
  assert.deepEqual(checkExperimentalGemmaRelease(assets, candidateRoot, approval), {
    releaseTag: 'semantic-v1',
    sourceRevision: 'source',
  });
  assert.throws(
    () =>
      checkExperimentalGemmaRelease(assets, candidateRoot, { ...approval, releaseTag: 'other' }),
    /releaseTag differs/u,
  );
  assert.throws(
    () =>
      checkExperimentalGemmaRelease(assets, candidateRoot, { ...approval, qualificationRunId: 9 }),
    /qualificationRunId differs/u,
  );
  const changed = {
    ...approval,
    targets: {
      ...approval.targets,
      'macos-aarch64': { ...approval.targets['macos-aarch64'], catalogSha256: 'changed' },
    },
  };
  assert.throws(
    () => checkExperimentalGemmaRelease(assets, candidateRoot, changed),
    /catalog differs/u,
  );
  writeFileSync(join(assets, 'embeddinggemma2-original.bin'), 'replaced');
  assert.throws(
    () => checkExperimentalGemmaRelease(assets, candidateRoot, approval),
    /SHA-256 does not match catalog/u,
  );
  writeFileSync(join(assets, 'embeddinggemma2-original.bin'), 'model.safetensors');
  writeFileSync(join(assets, 'semantic-catalog-windows-x86_64.sig'), 'different-signature');
  assert.throws(
    () => checkExperimentalGemmaRelease(assets, candidateRoot, approval),
    /signature SHA-256 does not match approval/u,
  );
  writeFileSync(join(assets, 'semantic-catalog-windows-x86_64.sig'), 'signature');
  writeFileSync(join(candidateRoot, 'semantic-component-release.json'), '{}');
  assert.throws(
    () => checkExperimentalGemmaRelease(assets, candidateRoot, approval),
    /no-go evidence/u,
  );
});

test('E5 report cannot silently become Gemma quality evidence', (context) => {
  const { assets, candidateRoot, approval } = fixture(context);
  const reportPath = join(assets, 'semantic-production-evaluation.json');
  const report = JSON.parse(readFileSync(reportPath));
  report.measurements[0].identity.pipeline.model.model = 'google-embeddinggemma-2';
  writeFileSync(reportPath, `${JSON.stringify(report)}\n`);
  const changed = {
    ...approval,
    experimentalGemma: {
      ...approval.experimentalGemma,
      evaluationSha256: sha(readFileSync(reportPath)),
    },
  };
  assert.throws(
    () => checkExperimentalGemmaRelease(assets, candidateRoot, changed),
    /no-go evidence/u,
  );
});

test('missing Gemma originals or a different model revision fail closed', (context) => {
  const { assets, candidateRoot, approval } = fixture(context);
  const changed = {
    ...approval,
    experimentalGemma: { ...approval.experimentalGemma, modelRevision: '0'.repeat(40) },
  };
  assert.throws(
    () => checkExperimentalGemmaRelease(assets, candidateRoot, changed),
    /model identity differs/u,
  );
  const catalogPath = join(assets, 'semantic-catalog-linux-x86_64.json');
  const catalog = JSON.parse(readFileSync(catalogPath));
  catalog.catalog.models = [];
  writeFileSync(catalogPath, `${JSON.stringify(catalog)}\n`);
  assert.throws(
    () => checkExperimentalGemmaRelease(assets, candidateRoot, approval),
    /catalog SHA-256 does not match approval/u,
  );
});

test('the historical pre-Metal candidate is explicitly blocked from publication', (context) => {
  const lock = JSON.parse(
    readFileSync(
      fileURLToPath(
        new URL('../docs/evaluations/semantic-gemma-experimental-v1.json', import.meta.url),
      ),
      'utf8',
    ),
  );
  assert.equal(lock.decision, 'noGo');
  assert.equal(lock.experimentalGemma.approval, 'blocked-stale-pre-integration-candidate');
  const { assets, candidateRoot } = fixture(context);
  assert.throws(
    () => checkExperimentalGemmaRelease(assets, candidateRoot, lock),
    /no-go evidence/u,
  );
  const oldCandidate = JSON.parse(
    readFileSync(join(candidateRoot, 'semantic-component-release.json'), 'utf8'),
  );
  oldCandidate.sourceRevision = lock.sourceRevision;
  const bytes = `${JSON.stringify(oldCandidate, null, 2)}\n`;
  writeFileSync(join(candidateRoot, 'semantic-component-release.json'), bytes);
  assert.throws(
    () =>
      checkExperimentalGemmaRelease(assets, candidateRoot, {
        ...lock,
        decision: 'go',
        experimentalGemma: {
          ...lock.experimentalGemma,
          approval: 'explicit-opt-in-experimental',
          candidateManifestSha256: sha(bytes),
        },
      }),
    /pre-integration Gemma worker cannot publish/u,
  );
});
