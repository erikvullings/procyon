import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { createSemanticComponentReleaseManifest } from './create-semantic-component-release-manifest.mjs';
import { semanticComponentReleasePlan } from './fetch-approved-semantic-catalog.mjs';
import {
  checkRiskAcceptedGemmaDesktopRelease,
  checkRiskAcceptedGemmaRelease,
  riskAcceptedGemmaLimitations,
  validateRiskAcceptedGemmaApproval,
} from './gemma-risk-acceptance.mjs';

const targets = ['linux-aarch64', 'linux-x86_64', 'macos-aarch64', 'windows-x86_64'];
const modelRevision = '9'.repeat(40);

function sha256(bytes) {
  return createHash('sha256').update(bytes).digest('hex');
}

function fixture(context, sourceRevision = '1'.repeat(40)) {
  const root = mkdtempSync(join(tmpdir(), 'gemma-risk-accepted-'));
  context.after(() => rmSync(root, { recursive: true, force: true }));
  const assets = join(root, 'assets');
  const candidateRoot = join(root, 'candidate');
  mkdirSync(assets);
  mkdirSync(candidateRoot);
  const components = [
    ['embeddinggemma2-original', 'model.safetensors'],
    ['embeddinggemma2-file-1', 'tokenizer.json'],
    ['embeddinggemma2-file-2', 'config.json'],
    ['embeddinggemma2-file-3', 'processor_config.json'],
    ['embeddinggemma2-file-4', 'preprocessor_config.json'],
    ['procyon.semantic.worker', 'worker'],
  ];
  const artifacts = components.map(([component, name]) => ({
    component,
    id: `${component}.bin`,
    name,
  }));
  for (const artifact of artifacts) writeFileSync(join(assets, artifact.id), artifact.name);
  for (const target of targets) {
    const catalog = {
      catalog: {
        revision: `catalog-${target}`,
        models: [
          {
            artifact_id: 'embeddinggemma2-original.bin',
            primary_file_name: 'model.safetensors',
            files: Object.fromEntries(
              components.slice(1, 5).map(([component, name]) => [name, `${component}.bin`]),
            ),
            metadata: {
              identity: { model: 'google-embeddinggemma-2', revision: modelRevision },
            },
          },
        ],
        artifacts: artifacts.map((artifact) => ({
          id: artifact.id,
          component_id: artifact.component,
          location: `https://github.com/erikvullings/procyon/releases/download/semantic-v1/${artifact.id}`,
          checksum: [...Buffer.from(sha256(artifact.name), 'hex')],
          resources: { download_bytes: Buffer.byteLength(artifact.name) },
        })),
      },
      provenance: [
        {
          artifact_id: 'procyon.semantic.worker.bin',
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
    evaluation: { decision: 'noGo', releaseCandidateFingerprint: 'sha256:fixture' },
    repository: 'erikvullings/procyon',
    releaseTag: 'semantic-v1',
    qualificationRunId: 42,
    sourceRevision,
  });
  const candidateBytes = Buffer.from(`${JSON.stringify(candidate, null, 2)}\n`);
  writeFileSync(join(candidateRoot, 'semantic-component-release.json'), candidateBytes);
  const approval = {
    ...candidate,
    schemaVersion: 1,
    decision: 'go',
    riskAcceptedGemma: {
      approval: 'release-owner-risk-accepted-unqualified',
      candidateDecision: candidate.decision,
      qualificationStatus: 'not-five-gate-qualified',
      modelRevision,
      candidateManifestSha256: sha256(candidateBytes),
      approvedBy: 'release-owner',
      approvedAt: '2026-10-18T09:00:00Z',
      acceptedLimitations: riskAcceptedGemmaLimitations,
    },
  };
  return { assets, candidateRoot, candidate, candidateBytes, approval };
}

test('risk acceptance explicitly records unqualified status and accepted limitations', (context) => {
  const { approval } = fixture(context);
  assert.equal(
    validateRiskAcceptedGemmaApproval(approval).qualificationStatus,
    'not-five-gate-qualified',
  );
  for (const mutate of [
    (manifest) => {
      delete manifest.riskAcceptedGemma;
    },
    (manifest) => {
      manifest.riskAcceptedGemma.qualificationStatus = 'qualified';
    },
    (manifest) => {
      manifest.riskAcceptedGemma.acceptedLimitations.pop();
    },
    (manifest) => {
      manifest.riskAcceptedGemma.acceptedLimitations.push(
        manifest.riskAcceptedGemma.acceptedLimitations[0],
      );
    },
    (manifest) => {
      manifest.nonExperimentalGemma = {};
    },
  ]) {
    const changed = structuredClone(approval);
    mutate(changed);
    assert.throws(() => validateRiskAcceptedGemmaApproval(changed), /risk acceptance/u);
  }
});

test('risk-accepted catalogs are usable only with the matching explicit manifest', (context) => {
  const { approval } = fixture(context);
  assert.equal(
    semanticComponentReleasePlan(approval, 'windows-x86_64').gemmaModelRevision,
    modelRevision,
  );
  const incompleteApproval = structuredClone(approval);
  incompleteApproval.riskAcceptedGemma.acceptedLimitations.pop();
  assert.throws(
    () => semanticComponentReleasePlan(incompleteApproval, 'windows-x86_64'),
    /risk acceptance/u,
  );
  const conflictingModes = structuredClone(approval);
  conflictingModes.experimentalGemma = {};
  assert.throws(
    () => semanticComponentReleasePlan(conflictingModes, 'windows-x86_64'),
    /modes cannot be combined/u,
  );
});

test('risk-accepted desktop packaging is opt-in and bound to signed targets', (context) => {
  const { approval } = fixture(context);
  assert.throws(
    () =>
      checkRiskAcceptedGemmaDesktopRelease(
        approval,
        'true',
        'false',
        'macos-aarch64',
        '1'.repeat(40),
      ),
    /independent semantic and owner approvals/u,
  );
  assert.throws(
    () =>
      checkRiskAcceptedGemmaDesktopRelease(
        approval,
        'true',
        'true',
        'macos-universal',
        '1'.repeat(40),
      ),
    /not in the signed component release/u,
  );
  assert.throws(
    () =>
      checkRiskAcceptedGemmaDesktopRelease(approval, 'true', 'true', 'macos-aarch64', 'invalid'),
    /desktop release source/u,
  );
});

test('risk-accepted desktop source must preserve the signed worker inputs', (context) => {
  const repositoryRoot = mkdtempSync(join(tmpdir(), 'gemma-risk-source-'));
  context.after(() => rmSync(repositoryRoot, { recursive: true, force: true }));
  execFileSync('git', ['init', '-q'], { cwd: repositoryRoot });
  execFileSync('git', ['config', 'user.email', 'test@example.invalid'], { cwd: repositoryRoot });
  execFileSync('git', ['config', 'user.name', 'Test'], { cwd: repositoryRoot });
  mkdirSync(join(repositoryRoot, 'crates/fm-semantic-worker/src'), { recursive: true });
  writeFileSync(join(repositoryRoot, 'Cargo.toml'), '[workspace]\n');
  writeFileSync(
    join(repositoryRoot, 'crates/fm-semantic-worker/src/lib.rs'),
    'pub fn worker() {}\n',
  );
  execFileSync('git', ['add', '.'], { cwd: repositoryRoot });
  execFileSync('git', ['commit', '-qm', 'Signed Gemma worker candidate'], { cwd: repositoryRoot });
  const sourceRevision = execFileSync('git', ['rev-parse', 'HEAD'], {
    cwd: repositoryRoot,
    encoding: 'utf8',
  }).trim();
  const { approval } = fixture(context, sourceRevision);

  writeFileSync(join(repositoryRoot, 'release-notes.txt'), 'release metadata\n');
  execFileSync('git', ['add', '.'], { cwd: repositoryRoot });
  execFileSync('git', ['commit', '-qm', 'Add release metadata'], { cwd: repositoryRoot });
  const releaseRevision = execFileSync('git', ['rev-parse', 'HEAD'], {
    cwd: repositoryRoot,
    encoding: 'utf8',
  }).trim();
  assert.doesNotThrow(() =>
    checkRiskAcceptedGemmaDesktopRelease(
      approval,
      'true',
      'true',
      'macos-aarch64',
      releaseRevision,
      repositoryRoot,
    ),
  );
  assert.doesNotThrow(() =>
    checkRiskAcceptedGemmaDesktopRelease(
      approval,
      'true',
      'true',
      'windows-x86_64',
      releaseRevision,
      repositoryRoot,
    ),
  );

  writeFileSync(
    join(repositoryRoot, 'crates/fm-semantic-worker/src/lib.rs'),
    'pub fn worker() { todo!() }\n',
  );
  execFileSync('git', ['add', '.'], { cwd: repositoryRoot });
  execFileSync('git', ['commit', '-qm', 'Change worker after candidate'], { cwd: repositoryRoot });
  const changedWorkerRevision = execFileSync('git', ['rev-parse', 'HEAD'], {
    cwd: repositoryRoot,
    encoding: 'utf8',
  }).trim();
  assert.throws(
    () =>
      checkRiskAcceptedGemmaDesktopRelease(
        approval,
        'true',
        'true',
        'macos-aarch64',
        changedWorkerRevision,
        repositoryRoot,
      ),
    /build inputs changed after signed candidate/u,
  );
});

test('risk acceptance approves only the exact candidate manifest, run, tag, source, and targets', (context) => {
  const { assets, candidateRoot, candidate, candidateBytes, approval } = fixture(context);
  assert.deepEqual(
    checkRiskAcceptedGemmaRelease(assets, candidateRoot, approval, 42, 'semantic-v1'),
    { releaseTag: 'semantic-v1', sourceRevision: candidate.sourceRevision },
  );

  const signaturePath = join(assets, 'semantic-catalog-linux-aarch64.sig');
  writeFileSync(signaturePath, 'changed signature');
  assert.throws(
    () => checkRiskAcceptedGemmaRelease(assets, candidateRoot, approval, 42, 'semantic-v1'),
    /signature SHA-256 does not match approval/u,
  );
  writeFileSync(signaturePath, 'signature-linux-aarch64');
  const payloadPath = join(assets, 'embeddinggemma2-original.bin');
  writeFileSync(payloadPath, 'tampered model');
  assert.throws(
    () => checkRiskAcceptedGemmaRelease(assets, candidateRoot, approval, 42, 'semantic-v1'),
    /payload embeddinggemma2-original\.bin SHA-256 does not match catalog/u,
  );
  writeFileSync(payloadPath, 'model.safetensors');

  const tamperedCandidate = { ...candidate, sourceRevision: '2'.repeat(40) };
  writeFileSync(
    join(candidateRoot, 'semantic-component-release.json'),
    `${JSON.stringify(tamperedCandidate, null, 2)}\n`,
  );
  assert.throws(
    () => checkRiskAcceptedGemmaRelease(assets, candidateRoot, approval, 42, 'semantic-v1'),
    /exact new signed candidate/u,
  );
  writeFileSync(join(candidateRoot, 'semantic-component-release.json'), candidateBytes);
  assert.throws(
    () => checkRiskAcceptedGemmaRelease(assets, candidateRoot, approval, 43, 'semantic-v1'),
    /exact new signed candidate/u,
  );
});
