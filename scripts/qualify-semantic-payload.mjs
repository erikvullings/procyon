import { spawnSync } from 'node:child_process';
import { randomBytes } from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { assertQualificationDispatchEnvironment } from './check-semantic-qualification-workflow.mjs';
import {
  scanSemanticPrivacyEvidence,
  verifySemanticPrivacyEvidence,
} from './semantic-privacy-scan.mjs';

const repositoryRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const bundle = path.resolve(process.argv[2] ?? '');
if (!process.argv[2]) {
  throw new Error('usage: qualify-semantic-payload.mjs <production-bundle>');
}
if (process.env.CI !== 'true') {
  throw new Error('semantic payload qualification is restricted to disposable CI runners');
}
assertQualificationDispatchEnvironment({
  eventName: process.env.GITHUB_EVENT_NAME,
  semanticReleaseQualified: process.env.SEMANTIC_RELEASE_QUALIFIED_VALUE,
  knowledgeSearchReleaseQualified: process.env.KNOWLEDGE_SEARCH_RELEASE_QUALIFIED_VALUE,
});

const qualificationRoot = path.join(bundle, 'qualification-worker');
const collected = path.join(qualificationRoot, 'collected');
if (fs.existsSync(qualificationRoot)) {
  throw new Error('semantic payload qualification output already exists');
}
fs.mkdirSync(collected, { recursive: true });
const catalogInput = JSON.parse(fs.readFileSync(path.join(bundle, 'catalog-input.json'), 'utf8'));

const categories = [
  'query',
  'excerpt',
  'filename-path',
  'prompt',
  'response',
  'credential',
  'token',
  'authorization-header',
  'model-payload',
];
const canaryMap = Object.fromEntries(
  categories.map((category) => [
    category,
    `procyon-${category}-${randomBytes(18).toString('hex')}`,
  ]),
);
const scanCanaries = Object.entries(canaryMap).map(([category, value]) => ({ category, value }));
const canaryFile = path.join(
  process.env.RUNNER_TEMP ?? path.dirname(bundle),
  `semantic-private-canaries-${randomBytes(8).toString('hex')}.json`,
);
fs.writeFileSync(canaryFile, JSON.stringify(canaryMap), { mode: 0o600, flag: 'wx' });
const result = spawnSync(
  process.execPath,
  [path.join(repositoryRoot, 'scripts', 'smoke-semantic-production-bundle.mjs'), bundle],
  {
    cwd: repositoryRoot,
    env: {
      ...process.env,
      PROCYON_QUALIFICATION_EVIDENCE_ROOT: path.join(collected, 'worker'),
      PROCYON_SEMANTIC_PRIVACY_CANARIES_FILE: canaryFile,
    },
    stdio: 'inherit',
  },
);
fs.rmSync(canaryFile, { force: true });
const captureStartedAt = new Date();
const capturePath = path.join(collected, 'diagnostics', 'authorized.capture');
fs.mkdirSync(path.dirname(capturePath), { recursive: true });
fs.writeFileSync(capturePath, `${canaryMap.prompt}\n${canaryMap.response}\n`, { mode: 0o600 });
const privacy = await scanSemanticPrivacyEvidence({
  root: collected,
  canaries: scanCanaries,
  captures: [
    {
      path: 'diagnostics/authorized.capture',
      categories: ['prompt', 'response'],
      previewedAt: captureStartedAt.toISOString(),
      expiresAt: new Date(captureStartedAt.valueOf() + 15 * 60 * 1000).toISOString(),
    },
  ],
  now: new Date(),
});
await verifySemanticPrivacyEvidence({ root: collected, report: privacy });
fs.writeFileSync(
  path.join(qualificationRoot, 'privacy-report.json'),
  `${JSON.stringify(privacy, null, 2)}\n`,
);
fs.cpSync(collected, path.join(qualificationRoot, 'safe-evidence'), {
  recursive: true,
  errorOnExist: true,
});
fs.writeFileSync(
  path.join(qualificationRoot, 'qualification-report.json'),
  `${JSON.stringify(
    {
      schemaVersion: 1,
      stage: 'packaged-worker-runtime',
      target: `${process.platform}-${process.arch}`,
      sourceRevision: process.env.GITHUB_SHA ?? 'unknown',
      runner: process.env.RUNNER_NAME ?? 'unknown',
      os: process.env.RUNNER_OS ?? process.platform,
      catalogRevision: catalogInput.catalog.revision,
      artifacts: catalogInput.catalog.artifacts.map((artifact) => ({
        id: artifact.id,
        componentId: artifact.component_id,
        bytes: artifact.resources.download_bytes,
        sha256: Buffer.from(artifact.checksum).toString('hex'),
      })),
      command: 'node scripts/smoke-semantic-production-bundle.mjs <private-bundle>',
      result: result.status === 0 && privacy.status === 'pass' ? 'pass' : 'fail',
      evidence: ['collected/worker', 'privacy-report.json'],
      crashArtifact: {
        status: 'unsupported',
        detail:
          'The qualification uses forced worker termination; the operating system does not guarantee a crash dump for that boundary.',
      },
      rollback:
        'Keep SEMANTIC_RELEASE_QUALIFIED false and restore the preceding immutable catalog and payloads.',
    },
    null,
    2,
  )}\n`,
);
if (result.error) throw result.error;
if (result.status !== 0) {
  throw new Error(`packaged worker qualification failed with status ${result.status ?? 'unknown'}`);
}
if (privacy.status !== 'pass') {
  throw new Error(`packaged worker privacy scan found ${privacy.findings.length} occurrence(s)`);
}
