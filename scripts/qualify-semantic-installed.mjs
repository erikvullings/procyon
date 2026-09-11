import { spawnSync } from 'node:child_process';
import { createHash, randomBytes } from 'node:crypto';
import {
  closeSync,
  createReadStream,
  existsSync,
  mkdirSync,
  openSync,
  readdirSync,
  readFileSync,
  writeFileSync,
} from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { assertQualificationDispatchEnvironment } from './check-semantic-qualification-workflow.mjs';
import {
  scanSemanticPrivacyEvidence,
  verifySemanticPrivacyEvidence,
} from './semantic-privacy-scan.mjs';

const repositoryRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

function argumentsFrom(values) {
  const argumentsMap = new Map();
  for (let index = 0; index < values.length; index += 2) {
    const name = values[index];
    const value = values[index + 1];
    if (!name?.startsWith('--') || !value) {
      throw new Error(
        'usage: qualify-semantic-installed.mjs --bundle <dir> --catalog <dir> --public-key <file> --evidence <dir>',
      );
    }
    argumentsMap.set(name, path.resolve(value));
  }
  for (const required of ['--bundle', '--catalog', '--public-key', '--evidence']) {
    if (!argumentsMap.has(required)) throw new Error(`missing ${required}`);
  }
  return argumentsMap;
}

function filesBelow(root) {
  if (!existsSync(root)) return [];
  const output = [];
  function visit(directory) {
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      const candidate = path.join(directory, entry.name);
      if (entry.isDirectory()) visit(candidate);
      else if (entry.isFile()) output.push(candidate);
    }
  }
  visit(root);
  return output.sort();
}

async function hashFile(file) {
  const digest = createHash('sha256');
  let bytes = 0;
  for await (const chunk of createReadStream(file)) {
    digest.update(chunk);
    bytes += chunk.length;
  }
  return { bytes, sha256: digest.digest('hex') };
}

async function packageEvidence() {
  const root = path.join(repositoryRoot, 'target', 'release', 'bundle');
  const packages = [];
  for (const file of filesBelow(root)) {
    if (!/\.(?:dmg|msi|exe|deb|appimage)$/iu.test(file)) continue;
    packages.push({
      path: path.relative(repositoryRoot, file).split(path.sep).join('/'),
      ...(await hashFile(file)),
    });
  }
  if (packages.length === 0) throw new Error('no native desktop package was produced');
  return packages;
}

function runStage(id, command, args, collected, environment = {}) {
  const log = path.join(collected, `${id}.log`);
  const output = openSync(log, 'w');
  const startedAt = new Date();
  const result = spawnSync(command, args, {
    cwd: repositoryRoot,
    env: { ...process.env, ...environment },
    stdio: ['ignore', output, output],
  });
  closeSync(output);
  return {
    id,
    status: result.status === 0 ? 'pass' : 'fail',
    stage: id,
    command: [command, ...args]
      .map((value) =>
        String(value)
          .replaceAll(repositoryRoot, '<repo>')
          .replaceAll(process.env.RUNNER_TEMP ?? '\0', '<runner-temp>'),
      )
      .join(' '),
    startedAt: startedAt.toISOString(),
    finishedAt: new Date().toISOString(),
    log: `collected/${path.basename(log)}`,
    exitCode: result.status,
    error: result.error?.message,
    rollback:
      'Keep SEMANTIC_RELEASE_QUALIFIED false and restore the preceding immutable signed catalog and payloads.',
  };
}

const args = argumentsFrom(process.argv.slice(2));
if (process.env.CI !== 'true') {
  throw new Error('installed semantic qualification is restricted to disposable CI runners');
}
assertQualificationDispatchEnvironment({
  eventName: process.env.GITHUB_EVENT_NAME,
  semanticReleaseQualified: process.env.SEMANTIC_RELEASE_QUALIFIED_VALUE,
  knowledgeSearchReleaseQualified: process.env.KNOWLEDGE_SEARCH_RELEASE_QUALIFIED_VALUE,
});

const bundle = args.get('--bundle');
const catalog = args.get('--catalog');
const publicKey = args.get('--public-key');
const evidence = args.get('--evidence');
if (existsSync(evidence))
  throw new Error('qualification evidence directory must not already exist');
const collected = path.join(evidence, 'collected');
mkdirSync(collected, { recursive: true });

const workerQualificationRoot = path.join(bundle, 'qualification-worker');
const workerPrivacyPath = path.join(workerQualificationRoot, 'privacy-report.json');
if (!existsSync(workerPrivacyPath)) {
  throw new Error('private payload is missing packaged-worker privacy evidence');
}
const workerPrivacy = JSON.parse(readFileSync(workerPrivacyPath, 'utf8'));
await verifySemanticPrivacyEvidence({
  root: path.join(workerQualificationRoot, 'collected'),
  report: workerPrivacy,
});
if (workerPrivacy.status !== 'pass') {
  throw new Error('private payload packaged-worker privacy evidence did not pass');
}

const canaryMap = Object.fromEntries(
  [
    'query',
    'excerpt',
    'filename-path',
    'prompt',
    'response',
    'credential',
    'token',
    'authorization-header',
    'model-payload',
  ].map((category) => [
    category,
    `procyon-installed-${category}-${randomBytes(18).toString('hex')}`,
  ]),
);
const filenameCanaryFile = path.join(
  process.env.RUNNER_TEMP ?? path.dirname(evidence),
  `semantic-filename-canary-${randomBytes(8).toString('hex')}.txt`,
);
writeFileSync(filenameCanaryFile, canaryMap['filename-path'], { mode: 0o600, flag: 'wx' });
const lifecycleRoot = path.join(
  process.env.RUNNER_TEMP ?? path.dirname(evidence),
  `semantic-lifecycle-${randomBytes(8).toString('hex')}`,
);
const stages = [];
stages.push(
  runStage(
    'exact-lifecycle',
    'cargo',
    [
      'run',
      '--quiet',
      '--locked',
      '-p',
      'fm-semantic-components',
      '--example',
      'qualify_semantic_lifecycle',
      '--',
      path.join(catalog, 'catalog.json'),
      path.join(catalog, 'catalog.sig'),
      path.join(bundle, 'artifacts'),
      publicKey,
      lifecycleRoot,
      path.join(collected, 'lifecycle-report.json'),
    ],
    collected,
  ),
);
stages.push(
  runStage(
    'diagnostic-capture',
    'cargo',
    [
      'test',
      '--quiet',
      '--locked',
      '-p',
      'fm-application',
      '--lib',
      'semantic_hardening::tests::diagnostic_capture_is_previewed_scoped_and_expires',
    ],
    collected,
  ),
);
stages.push(
  runStage(
    'cancellation-restart',
    'cargo',
    [
      'test',
      '--quiet',
      '--locked',
      '-p',
      'fm-application',
      '--test',
      'semantic',
      'semantic_cancel_targets_an_ingestion_operation_while_it_is_in_flight',
    ],
    collected,
  ),
);
stages.push(
  runStage(
    'installed-package',
    process.execPath,
    [path.join(repositoryRoot, 'scripts', 'smoke-desktop-package.mjs')],
    collected,
    {
      PROCYON_QUALIFICATION_EVIDENCE_ROOT: path.join(collected, 'package'),
      PROCYON_QUALIFICATION_FILENAME_CANARY_FILE: filenameCanaryFile,
      PROCYON_QUALIFICATION_CATALOG_DIRECTORY: catalog,
    },
  ),
);
fs.rmSync(filenameCanaryFile, { force: true });

const packages = await packageEvidence();
const catalogManifest = JSON.parse(readFileSync(path.join(catalog, 'catalog.json'), 'utf8'));
const captureStartedAt = new Date();
const capture = path.join(collected, 'diagnostics', 'authorized.capture');
mkdirSync(path.dirname(capture), { recursive: true });
writeFileSync(capture, `${canaryMap.query}\n${canaryMap.excerpt}\n`, { mode: 0o600 });

const qualificationReport = {
  schemaVersion: 1,
  decision: 'no-go',
  target: process.env.PROCYON_QUALIFICATION_TARGET ?? `${process.platform}-${process.arch}`,
  sourceRevision: process.env.GITHUB_SHA ?? 'unknown',
  runner: process.env.RUNNER_NAME ?? 'unknown',
  runnerOs: process.env.RUNNER_OS ?? process.platform,
  catalogRevision: catalogManifest.catalog.revision,
  catalogSha256: (await hashFile(path.join(catalog, 'catalog.json'))).sha256,
  signatureSha256: (await hashFile(path.join(catalog, 'catalog.sig'))).sha256,
  artifacts: catalogManifest.catalog.artifacts.map((artifact) => ({
    id: artifact.id,
    componentId: artifact.component_id,
    bytes: artifact.resources.download_bytes,
    sha256: Buffer.from(artifact.checksum).toString('hex'),
  })),
  packages,
  stages,
  inheritedWorkerPrivacy: {
    status: workerPrivacy.status,
    canaryCategories: workerPrivacy.canaryCategories,
    report: 'private payload qualification-worker/privacy-report.json',
  },
  manualChecklist: [
    { check: 'VoiceOver', status: 'manual-required' },
    { check: 'Narrator', status: 'manual-required' },
    { check: 'Orca', status: 'manual-required' },
    { check: 'keyboard-only installation and consent', status: 'manual-required' },
    { check: 'progress, cancellation, and error announcements', status: 'manual-required' },
    { check: 'citation opening and focus restoration', status: 'manual-required' },
    { check: 'retention and explicit deletion comprehension', status: 'manual-required' },
  ],
  blockers: [
    {
      check: 'preceding production candidate upgrade and rollback',
      status: 'blocked',
      detail: 'No exact signed preceding production candidate was supplied to this run.',
    },
    {
      check: 'task-0188 production retrieval and grounded-answer evaluation',
      status: 'blocked',
    },
    {
      check: 'release-owner approval',
      status: 'blocked',
    },
  ],
  unsupported: [
    {
      check: 'macOS x86-64 semantic runtime',
      status: 'unsupported',
      detail: 'Zvec 0.7.0 publishes no matching runtime.',
    },
    {
      check: 'guaranteed operating-system crash dump after forced termination',
      status: 'unsupported',
      detail: 'The platform does not guarantee a dump for the forced worker termination boundary.',
    },
    {
      check: 'semantic updater log',
      status: 'unsupported',
      detail:
        'Procyon has no automatic semantic updater; component lifecycle evidence is recorded instead.',
    },
  ],
  rollback:
    'Leave both release gates false. Restore the preceding immutable signed catalog and payload set; retain semantic indexes unless the user explicitly selects deletion.',
};
writeFileSync(
  path.join(collected, 'qualification-report.json'),
  `${JSON.stringify(qualificationReport, null, 2)}\n`,
);

const privacy = await scanSemanticPrivacyEvidence({
  root: collected,
  canaries: Object.entries(canaryMap).map(([category, value]) => ({ category, value })),
  captures: [
    {
      path: 'diagnostics/authorized.capture',
      categories: ['query', 'excerpt'],
      previewedAt: captureStartedAt.toISOString(),
      expiresAt: new Date(captureStartedAt.valueOf() + 15 * 60 * 1000).toISOString(),
    },
  ],
  now: new Date(),
});
await verifySemanticPrivacyEvidence({ root: collected, report: privacy });
writeFileSync(path.join(evidence, 'privacy-report.json'), `${JSON.stringify(privacy, null, 2)}\n`);

const failed = stages.filter(({ status }) => status !== 'pass');
const summary = {
  schemaVersion: 1,
  decision: 'no-go',
  automatedStatus: failed.length === 0 && privacy.status === 'pass' ? 'pass' : 'fail',
  target: qualificationReport.target,
  sourceRevision: qualificationReport.sourceRevision,
  catalogRevision: qualificationReport.catalogRevision,
  stageResults: stages.map(({ id, status, log, exitCode }) => ({ id, status, log, exitCode })),
  privacyStatus: privacy.status,
  workerPrivacyStatus: workerPrivacy.status,
  manualRequired: qualificationReport.manualChecklist.map(({ check }) => check),
  blockers: qualificationReport.blockers,
  rollback: qualificationReport.rollback,
};
writeFileSync(path.join(evidence, 'summary.json'), `${JSON.stringify(summary, null, 2)}\n`);

if (failed.length > 0) {
  throw new Error(
    `installed semantic qualification failed at ${failed.map(({ id }) => id).join(', ')}`,
  );
}
if (privacy.status !== 'pass') {
  throw new Error(`installed privacy scan found ${privacy.findings.length} occurrence(s)`);
}
