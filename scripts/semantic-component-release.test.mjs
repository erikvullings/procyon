import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import {
  checkDesktopGemmaRelease,
  semanticWorkerLockClosure,
  verifySemanticWorkerSource,
} from './check-desktop-gemma-release.mjs';
import {
  createSemanticComponentReleaseManifest,
  semanticCatalogSourceRevision,
} from './create-semantic-component-release-manifest.mjs';
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
          component_id: 'procyon.semantic.worker',
          location:
            'https://github.com/erikvullings/procyon/releases/download/semantic-v1/worker.abc',
          checksum: [...Buffer.from(sha256(payload), 'hex')],
          resources: { download_bytes: payload.byteLength },
        },
      ],
    },
    provenance: [
      {
        artifact_id: 'worker.abc',
        source: 'https://github.com/erikvullings/procyon',
        source_revision: 'abc123',
      },
    ],
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

test('experimental desktop requires opt-in and an approved model revision', () => {
  const approval = JSON.parse(
    readFileSync(
      new URL('../docs/evaluations/semantic-gemma-experimental-v1.json', import.meta.url),
    ),
  );
  assert.throws(
    () => checkDesktopGemmaRelease(approval, 'false', 'true'),
    /both independent release approvals/u,
  );
  assert.deepEqual(checkDesktopGemmaRelease(approval, 'true', 'true'), approval);
  const targets = Object.fromEntries(
    ['linux-aarch64', 'linux-x86_64', 'macos-aarch64', 'windows-x86_64'].map((target) => [
      target,
      manifest.targets['linux-x86_64'],
    ]),
  );
  const proposed = {
    ...manifest,
    sourceRevision: 'a'.repeat(40),
    targets,
    experimentalGemma: {
      approval: 'explicit-opt-in-experimental',
      candidateDecision: 'noGo',
      modelRevision: 'b'.repeat(40),
    },
  };
  assert.throws(() => checkDesktopGemmaRelease(proposed, 'false', 'true'), /both independent/u);
  assert.throws(() => checkDesktopGemmaRelease(proposed, 'true', 'false'), /both independent/u);
  // A universal Mac app is allowed: its Intel slice disables production semantic search.
  assert.doesNotThrow(() => checkDesktopGemmaRelease(proposed, 'true', 'true'));
  assert.throws(
    () =>
      checkDesktopGemmaRelease(
        {
          ...proposed,
          targets: { ...targets, 'macos-x86_64': manifest.targets['linux-x86_64'] },
        },
        'true',
        'true',
      ),
    /approved four-target experimental Gemma release/u,
  );
  assert.throws(
    () => checkDesktopGemmaRelease({ ...proposed, experimentalGemma: undefined }, 'true', 'true'),
    /explicit experimental/u,
  );
  assert.throws(
    () =>
      checkDesktopGemmaRelease(
        { ...proposed, sourceRevision: 'd3f921fe08e4994a905727a71af102979f4fbe23' },
        'true',
        'true',
      ),
    /new, explicitly approved/u,
  );
  assert.throws(
    () =>
      verifySemanticCatalogBytes(
        semanticComponentReleasePlan(proposed, 'linux-x86_64'),
        catalog,
        signature,
      ),
    /catalog SHA-256/u,
  );
  const matchingSource = Buffer.from(
    catalog
      .toString()
      .replace('"source_revision":"abc123"', `"source_revision":"${'a'.repeat(40)}"`),
  );
  const matching = {
    ...proposed,
    targets: {
      ...targets,
      'linux-x86_64': {
        ...targets['linux-x86_64'],
        catalogSha256: sha256(matchingSource),
      },
    },
  };
  assert.throws(
    () =>
      verifySemanticCatalogBytes(
        semanticComponentReleasePlan(matching, 'linux-x86_64'),
        matchingSource,
        signature,
      ),
    /model identity differs/u,
  );
});

test('desktop release allows reviewed manifest commits but rejects worker source changes', (context) => {
  const root = mkdtempSync(join(tmpdir(), 'semantic-worker-source-'));
  context.after(() => rmSync(root, { force: true, recursive: true }));
  execFileSync('git', ['init', '-q'], { cwd: root });
  execFileSync('git', ['config', 'user.email', 'test@example.invalid'], { cwd: root });
  execFileSync('git', ['config', 'user.name', 'Test'], { cwd: root });
  mkdirSync(join(root, 'crates/fm-semantic-worker/src'), { recursive: true });
  mkdirSync(join(root, 'docs/evaluations'), { recursive: true });
  writeFileSync(join(root, 'Cargo.toml'), '[workspace]\n');
  writeFileSync(join(root, 'crates/fm-semantic-worker/src/lib.rs'), 'pub fn worker() {}\n');
  writeFileSync(join(root, 'docs/evaluations/semantic-gemma-experimental-v1.json'), '{}\n');
  execFileSync('git', ['add', '.'], { cwd: root });
  execFileSync('git', ['commit', '-qm', 'Build signed candidate'], { cwd: root });
  const candidateRevision = execFileSync('git', ['rev-parse', 'HEAD'], {
    cwd: root,
    encoding: 'utf8',
  }).trim();

  writeFileSync(
    join(root, 'docs/evaluations/semantic-gemma-experimental-v1.json'),
    '{"qualificationRunId":123}\n',
  );
  execFileSync('git', ['add', '.'], { cwd: root });
  execFileSync('git', ['commit', '-qm', 'Record signed candidate approval'], { cwd: root });
  const releaseRevision = execFileSync('git', ['rev-parse', 'HEAD'], {
    cwd: root,
    encoding: 'utf8',
  }).trim();
  assert.doesNotThrow(() => verifySemanticWorkerSource(candidateRevision, releaseRevision, root));

  writeFileSync(
    join(root, 'crates/fm-semantic-worker/src/lib.rs'),
    'pub fn worker() { todo!() }\n',
  );
  execFileSync('git', ['add', '.'], { cwd: root });
  execFileSync('git', ['commit', '-qm', 'Change worker after candidate'], { cwd: root });
  const changedWorkerRevision = execFileSync('git', ['rev-parse', 'HEAD'], {
    cwd: root,
    encoding: 'utf8',
  }).trim();
  assert.throws(
    () => verifySemanticWorkerSource(candidateRevision, changedWorkerRevision, root),
    /build inputs changed after signed candidate/u,
  );
});

function lockPackage(name, version, dependencies = [], source = undefined) {
  const lines = ['[[package]]', `name = "${name}"`, `version = "${version}"`];
  if (source) lines.push(`source = "${source}"`, `checksum = "${name}-${version}-sum"`);
  if (dependencies.length > 0) {
    lines.push('dependencies = [', ...dependencies.map((dependency) => ` "${dependency}",`), ']');
  }
  return `${lines.join('\n')}\n`;
}

const registry = 'registry+https://github.com/rust-lang/crates.io-index';

function workerLock({ version, serde = '1.0.0', unrelated = '1.0.0' }) {
  return [
    lockPackage('fm-semantic-worker', version, ['fm-semantic-protocol', 'serde']),
    lockPackage('fm-semantic-protocol', version, ['serde']),
    lockPackage('fm-desktop', version, ['tauri']),
    lockPackage('serde', serde, [], registry),
    lockPackage('tauri', unrelated, [], registry),
  ].join('\n');
}

test('desktop release ignores version bumps, unrelated packages and tests but not worker inputs', (context) => {
  const root = mkdtempSync(join(tmpdir(), 'semantic-worker-inputs-'));
  context.after(() => rmSync(root, { force: true, recursive: true }));
  const run = (...args) => execFileSync('git', args, { cwd: root, encoding: 'utf8' }).trim();
  run('init', '-q');
  run('config', 'user.email', 'test@example.invalid');
  run('config', 'user.name', 'Test');
  for (const crate of ['fm-semantic-worker', 'fm-semantic-protocol', 'fm-desktop']) {
    mkdirSync(join(root, 'crates', crate, 'src'), { recursive: true });
    mkdirSync(join(root, 'crates', crate, 'tests'), { recursive: true });
    writeFileSync(join(root, 'crates', crate, 'src/lib.rs'), 'pub fn run() {}\n');
    writeFileSync(join(root, 'crates', crate, 'tests/it.rs'), '// test\n');
  }
  const manifest = (version, members) =>
    `[workspace]\nmembers = [${members}]\n\n[workspace.package]\nversion = "${version}"\n\n[workspace.dependencies]\nserde = "1"\n`;
  const commit = (message, files) => {
    for (const [file, content] of Object.entries(files)) writeFileSync(join(root, file), content);
    run('add', '.');
    run('commit', '-qm', message);
    return run('rev-parse', 'HEAD');
  };
  const signed = commit('Signed candidate', {
    'Cargo.toml': manifest('0.1.0', '"crates/*"'),
    'Cargo.lock': workerLock({ version: '0.1.0' }),
    'rust-toolchain.toml': '[toolchain]\nchannel = "1.0"\n',
  });

  const harmless = commit('Bump version, change unrelated code, tests and members', {
    'Cargo.toml': manifest('0.2.0', '"crates/*", "apps/*"'),
    'Cargo.lock': workerLock({ version: '0.2.0', unrelated: '2.0.0' }),
    'crates/fm-desktop/src/lib.rs': 'pub fn changed() {}\n',
    'crates/fm-semantic-worker/tests/it.rs': '// changed test\n',
  });
  assert.doesNotThrow(() => verifySemanticWorkerSource(signed, harmless, root));

  const dependency = commit('Update a worker dependency', {
    'Cargo.lock': workerLock({ version: '0.2.0', unrelated: '2.0.0', serde: '1.0.1' }),
  });
  assert.throws(
    () => verifySemanticWorkerSource(signed, dependency, root),
    /Cargo\.lock: serde 1\.0\.1|Cargo\.lock: serde 1\.0\.0/u,
  );

  const protocol = commit('Change a crate the worker depends on', {
    'Cargo.lock': workerLock({ version: '0.2.0', unrelated: '2.0.0' }),
    'crates/fm-semantic-protocol/src/lib.rs': 'pub fn changed() {}\n',
  });
  assert.throws(
    () => verifySemanticWorkerSource(signed, protocol, root),
    /crates\/fm-semantic-protocol\/src\/lib\.rs/u,
  );

  const toolchain = commit('Change the toolchain', {
    'rust-toolchain.toml': '[toolchain]\nchannel = "1.1"\n',
  });
  assert.throws(
    () => verifySemanticWorkerSource(harmless, toolchain, root),
    /rust-toolchain\.toml/u,
  );
});

test('worker lock closure covers only packages reachable from the worker', () => {
  const closure = semanticWorkerLockClosure(workerLock({ version: '0.1.0' }));
  assert.deepEqual([...closure.keys()].sort(), [
    'fm-semantic-protocol',
    'fm-semantic-worker',
    'serde 1.0.0',
  ]);
});

test('catalog source identity requires one matching Procyon worker provenance record', () => {
  const envelope = JSON.parse(catalog.toString());
  assert.equal(semanticCatalogSourceRevision(envelope, 'erikvullings/procyon'), 'abc123');
  assert.throws(
    () => semanticCatalogSourceRevision({ ...envelope, provenance: [] }, 'erikvullings/procyon'),
    /exactly one Procyon worker provenance/u,
  );
  assert.throws(
    () =>
      semanticCatalogSourceRevision(
        { ...envelope, provenance: [...envelope.provenance, ...envelope.provenance] },
        'erikvullings/procyon',
      ),
    /exactly one Procyon worker provenance/u,
  );
  assert.throws(
    () => semanticCatalogSourceRevision(envelope, 'other/repository'),
    /exactly one Procyon worker provenance/u,
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

test('Gemma payloads cannot publish without a Gemma-specific release decision', (context) => {
  const root = mkdtempSync(join(tmpdir(), 'semantic-component-gemma-gate-'));
  context.after(() => rmSync(root, { force: true, recursive: true }));
  writeFileSync(join(root, 'worker.abc'), payload);
  const envelope = JSON.parse(catalog.toString());
  envelope.catalog.models = [{ metadata: { identity: { model: 'google-embeddinggemma-2' } } }];
  assert.throws(
    () => verifySemanticPayloads(envelope, root, 'linux-x86_64'),
    /Gemma-specific release qualification is required/u,
  );
});

test('qualification emits a reviewable lock over every catalog and signature', (context) => {
  const root = mkdtempSync(join(tmpdir(), 'semantic-component-release-'));
  context.after(() => rmSync(root, { force: true, recursive: true }));
  for (const target of ['linux-aarch64', 'linux-x86_64', 'macos-aarch64', 'windows-x86_64']) {
    const targetCatalog = Buffer.from(
      catalog
        .toString()
        .replace('"catalog-linux"', `"catalog-${target}"`)
        .replaceAll('"worker.abc"', `"worker.${target}"`)
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
