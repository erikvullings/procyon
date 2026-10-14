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

function fixture(context, { pdfRenderer = true, pdfEvidence = true } = {}) {
  const root = mkdtempSync(join(tmpdir(), 'gemma-experimental-release-'));
  context.after(() => rmSync(root, { recursive: true, force: true }));
  const assets = join(root, 'assets');
  const candidateRoot = join(root, 'candidate');
  const evidenceRoot = join(root, 'pdf-evidence');
  mkdirSync(assets);
  mkdirSync(candidateRoot);
  mkdirSync(evidenceRoot);
  const sourceRevision = 'a'.repeat(40);
  const artifacts = [
    ...files.map(([component, name]) => ({ component, id: `${component}.bin`, content: name })),
    { component: 'procyon.semantic.worker', id: 'worker.bin', content: 'worker' },
    ...(pdfRenderer
      ? [
          {
            component: 'procyon.semantic.pdfium-runtime',
            id: 'pdfium-runtime.bin',
            content: 'native PDFium',
          },
          {
            component: 'procyon.semantic.pdfium-notices',
            id: 'pdfium-notices.txt',
            content: 'complete notices',
          },
        ]
      : []),
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
          ...(item.component === 'procyon.semantic.worker' && pdfRenderer
            ? {
                compatibility: {
                  runtimes: [
                    {
                      component_id: 'procyon.semantic.pdfium-runtime',
                      version_requirement: '=0.8086.0',
                    },
                  ],
                },
              }
            : {}),
        })),
      },
      provenance: [
        {
          artifact_id: 'worker.bin',
          source: 'https://github.com/o/r',
          source_revision: sourceRevision,
        },
        ...(pdfRenderer
          ? ['pdfium-runtime.bin', 'pdfium-notices.txt'].map((artifact_id) => ({
              artifact_id,
              source: 'https://github.com/bblanchon/pdfium-binaries',
              source_revision: '5325fa6d0d9379329f10f98fdc4839b4e40f2e79',
            }))
          : []),
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
    sourceRevision,
  });
  const candidateBytes = `${JSON.stringify(candidate, null, 2)}\n`;
  writeFileSync(join(candidateRoot, 'semantic-component-release.json'), candidateBytes);
  const pdfTargets = {};
  if (pdfEvidence) {
    for (const target of targets) {
      const directory = join(evidenceRoot, `installed-gemma-${target}`);
      mkdirSync(directory);
      const backend =
        target === 'macos-aarch64'
          ? 'google/embeddinggemma-2 (FP32 Metal)'
          : 'google/embeddinggemma-2 (native CPU FP32)';
      const imageEvidenceCount = backend.includes('CPU') ? 4 : 8;
      const evidence = {
        schemaVersion: 1,
        decision: 'noGo',
        scope: 'installed-host-vfs-8-page-synthetic-pdf-sample',
        target,
        qualificationRunId: '456',
        qualificationSourceRevision: sourceRevision,
        candidateRunId: 123,
        candidateSourceRevision: sourceRevision,
        catalogSha256: candidate.targets[target].catalogSha256,
        sourceBytes: 3_967_050,
        sourceSha256: '6fd090eee5fcdd7e547c8787f8ada4465e896fcff190b84e2b2f49e9c5b3d690',
        pageCount: 8,
        imageEvidenceCount,
        pdfImageSkippedPages: 8 - imageEvidenceCount,
        backend,
        completeGenerationVerified: true,
        hostVfsVerified: true,
        replacementVerified: true,
        fallbackVerified: true,
        upgradeRollbackVerified: true,
        currentContentAndDeletionVerified: true,
        crossHostNavigationVerified: true,
        resourceCeilingQualified: true,
        elapsedSeconds: 10_000,
        secondsPerPage: 1_250,
        pagesPerSecond: 8 / 10_000,
        secondsPerImagePage: 10_000 / imageEvidenceCount,
        imagePagesPerSecond: imageEvidenceCount / 10_000,
        releaseCeilingSeconds: 12_000,
        hostPeakRssBytes: 200_000_000,
        workerPeakRssBytes: 800_000_000,
        temporaryDiskPeakBytes: 300_000_000,
        qualityDisclosure: 'experimental-unproven',
      };
      const bytes = `${JSON.stringify(evidence)}\n`;
      writeFileSync(join(directory, 'gemma-pdf-bound-report.json'), bytes);
      pdfTargets[target] = sha(bytes);
    }
  }
  const approval = {
    ...candidate,
    decision: 'go',
    experimentalGemma: {
      approval: 'explicit-opt-in-experimental',
      candidateDecision: 'noGo',
      candidateManifestSha256: sha(candidateBytes),
      evaluationSha256: sha(readFileSync(join(assets, 'semantic-production-evaluation.json'))),
      modelRevision: '914f7f89142e33e77833254d9c9b90c3cef7303b',
      ...(pdfEvidence
        ? { pdfVisionQualification: { runId: 456, sourceRevision, targets: pdfTargets } }
        : {}),
    },
  };
  return { assets, candidateRoot, evidenceRoot, approval };
}

test('only the exact four-target signed candidate and E5 negative-control report pass', (context) => {
  const { assets, candidateRoot, evidenceRoot, approval } = fixture(context);
  const check = (decision) =>
    checkExperimentalGemmaRelease(assets, candidateRoot, decision, evidenceRoot);
  assert.deepEqual(check(approval), {
    releaseTag: 'semantic-v1',
    sourceRevision: 'a'.repeat(40),
  });
  assert.throws(() => check({ ...approval, releaseTag: 'other' }), /releaseTag differs/u);
  assert.throws(() => check({ ...approval, qualificationRunId: 9 }), /qualificationRunId differs/u);
  const changed = {
    ...approval,
    targets: {
      ...approval.targets,
      'macos-aarch64': { ...approval.targets['macos-aarch64'], catalogSha256: 'changed' },
    },
  };
  assert.throws(() => check(changed), /catalog differs/u);
  writeFileSync(join(assets, 'embeddinggemma2-original.bin'), 'replaced');
  assert.throws(() => check(approval), /SHA-256 does not match catalog/u);
  writeFileSync(join(assets, 'embeddinggemma2-original.bin'), 'model.safetensors');
  writeFileSync(join(assets, 'semantic-catalog-windows-x86_64.sig'), 'different-signature');
  assert.throws(() => check(approval), /signature SHA-256 does not match approval/u);
  writeFileSync(join(assets, 'semantic-catalog-windows-x86_64.sig'), 'signature');
  writeFileSync(join(candidateRoot, 'semantic-component-release.json'), '{}');
  assert.throws(() => check(approval), /no-go evidence/u);
});

test('E5 report cannot silently become Gemma quality evidence', (context) => {
  const { assets, candidateRoot, evidenceRoot, approval } = fixture(context);
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
    () => checkExperimentalGemmaRelease(assets, candidateRoot, changed, evidenceRoot),
    /no-go evidence/u,
  );
});

test('missing Gemma originals or a different model revision fail closed', (context) => {
  const { assets, candidateRoot, evidenceRoot, approval } = fixture(context);
  const changed = {
    ...approval,
    experimentalGemma: { ...approval.experimentalGemma, modelRevision: '0'.repeat(40) },
  };
  assert.throws(
    () => checkExperimentalGemmaRelease(assets, candidateRoot, changed, evidenceRoot),
    /model identity differs/u,
  );
  const catalogPath = join(assets, 'semantic-catalog-linux-x86_64.json');
  const catalog = JSON.parse(readFileSync(catalogPath));
  catalog.catalog.models = [];
  writeFileSync(catalogPath, `${JSON.stringify(catalog)}\n`);
  assert.throws(
    () => checkExperimentalGemmaRelease(assets, candidateRoot, approval, evidenceRoot),
    /catalog SHA-256 does not match approval/u,
  );
});

test('experimental Gemma refuses signed candidates without PDF renderer and notices on every target', (context) => {
  const { assets, candidateRoot, evidenceRoot, approval } = fixture(context, {
    pdfRenderer: false,
  });
  assert.throws(
    () => checkExperimentalGemmaRelease(assets, candidateRoot, approval, evidenceRoot),
    /PDF renderer and notices/u,
  );
});

test('signed PDFium alone cannot authorize a Gemma release without installed sampled evidence', (context) => {
  const { assets, candidateRoot, approval } = fixture(context, { pdfEvidence: false });
  assert.throws(
    () => checkExperimentalGemmaRelease(assets, candidateRoot, approval),
    /PDF vision qualification/u,
  );
});

test('CPU release evidence must honor the four-page cap and report skipped pages', (context) => {
  const { assets, candidateRoot, evidenceRoot, approval } = fixture(context);
  const file = join(evidenceRoot, 'installed-gemma-linux-aarch64', 'gemma-pdf-bound-report.json');
  const report = JSON.parse(readFileSync(file));
  report.imageEvidenceCount = 8;
  report.pdfImageSkippedPages = 0;
  const bytes = `${JSON.stringify(report)}\n`;
  writeFileSync(file, bytes);
  const changedApproval = {
    ...approval,
    experimentalGemma: {
      ...approval.experimentalGemma,
      pdfVisionQualification: {
        ...approval.experimentalGemma.pdfVisionQualification,
        targets: {
          ...approval.experimentalGemma.pdfVisionQualification.targets,
          'linux-aarch64': sha(bytes),
        },
      },
    },
  };
  assert.throws(
    () => checkExperimentalGemmaRelease(assets, candidateRoot, changedApproval, evidenceRoot),
    /PDF vision qualification is incomplete/u,
  );
});

test('changed or incomplete PDF evidence cannot be promoted by updating only its digest', (context) => {
  const { assets, candidateRoot, evidenceRoot, approval } = fixture(context);
  const target = 'macos-aarch64';
  const file = join(evidenceRoot, `installed-gemma-${target}`, 'gemma-pdf-bound-report.json');
  const report = JSON.parse(readFileSync(file));
  report.resourceCeilingQualified = false;
  const bytes = `${JSON.stringify(report)}\n`;
  writeFileSync(file, bytes);
  assert.throws(
    () => checkExperimentalGemmaRelease(assets, candidateRoot, approval, evidenceRoot),
    /PDF vision qualification is incomplete/u,
  );
  const changed = {
    ...approval,
    experimentalGemma: {
      ...approval.experimentalGemma,
      pdfVisionQualification: {
        ...approval.experimentalGemma.pdfVisionQualification,
        targets: {
          ...approval.experimentalGemma.pdfVisionQualification.targets,
          [target]: sha(bytes),
        },
      },
    },
  };
  assert.throws(
    () => checkExperimentalGemmaRelease(assets, candidateRoot, changed, evidenceRoot),
    /PDF vision qualification is incomplete/u,
  );
});

test('the mixed-media Metal candidate lock remains blocked and the pre-integration source cannot publish', (context) => {
  const lock = JSON.parse(
    readFileSync(
      fileURLToPath(
        new URL('../docs/evaluations/semantic-gemma-experimental-v1.json', import.meta.url),
      ),
      'utf8',
    ),
  );
  assert.equal(lock.decision, 'noGo');
  assert.equal(lock.sourceRevision, '021e8ab9a40fa98591ba997d700ad501693b8e2b');
  assert.equal(lock.experimentalGemma.approval, 'blocked-pending-release-owner-review');
  const { assets, candidateRoot, evidenceRoot } = fixture(context);
  assert.throws(
    () => checkExperimentalGemmaRelease(assets, candidateRoot, lock, evidenceRoot),
    /no-go evidence/u,
  );
  const oldCandidate = JSON.parse(
    readFileSync(join(candidateRoot, 'semantic-component-release.json'), 'utf8'),
  );
  oldCandidate.sourceRevision = 'd3f921fe08e4994a905727a71af102979f4fbe23';
  const bytes = `${JSON.stringify(oldCandidate, null, 2)}\n`;
  writeFileSync(join(candidateRoot, 'semantic-component-release.json'), bytes);
  assert.throws(
    () =>
      checkExperimentalGemmaRelease(
        assets,
        candidateRoot,
        {
          ...lock,
          decision: 'go',
          experimentalGemma: {
            ...lock.experimentalGemma,
            approval: 'explicit-opt-in-experimental',
            candidateManifestSha256: sha(bytes),
          },
        },
        evidenceRoot,
      ),
    /pre-integration Gemma worker cannot publish/u,
  );
});
