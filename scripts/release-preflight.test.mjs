import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { semanticWorkerChanges } from './check-desktop-gemma-release.mjs';
import { checkSemanticQualificationWorkflow } from './check-semantic-qualification-workflow.mjs';
import { riskAcceptedGemmaManifestPath } from './gemma-risk-acceptance.mjs';
import { prepareRiskAcceptedGemmaManifest } from './prepare-risk-accepted-gemma-manifest.mjs';
import {
  activeGemmaModes,
  classifyGateFailure,
  classifyQualificationRuns,
  formatReport,
  remedies,
  runPreflight,
  workspaceVersion,
} from './release-preflight.mjs';
import { resolveReleaseTag } from './resign-gemma-candidate.mjs';
import { planSemanticQualification } from './semantic-qualification-plan.mjs';
import { autoQualificationTag, nextSemanticTag } from './semantic-release-tags.mjs';

const current = JSON.parse(fs.readFileSync(riskAcceptedGemmaManifestPath, 'utf8'));
const goodVars = {
  SEMANTIC_RELEASE_QUALIFIED: 'true',
  SEMANTIC_GEMMA_RISK_ACCEPTED: 'true',
};

/**
 * A repository with a worker crate, a worker lock closure and a risk-accepted manifest committed
 * on top of the candidate commit, mirroring how the real signed candidate flow lays out history.
 */
function fixture(context) {
  const root = fs.mkdtempSync(join(tmpdir(), 'release-preflight-'));
  context.after(() => fs.rmSync(root, { force: true, recursive: true }));
  const git = (...args) => execFileSync('git', args, { cwd: root, encoding: 'utf8' }).trim();
  const write = (file, content) => {
    fs.mkdirSync(join(root, file, '..'), { recursive: true });
    fs.writeFileSync(join(root, file), content);
  };
  const commit = (message, files) => {
    for (const [file, content] of Object.entries(files)) write(file, content);
    git('add', '.');
    git('commit', '-qm', message);
    return git('rev-parse', 'HEAD');
  };
  const cargo = (version) =>
    `[workspace]\nmembers = []\n\n[workspace.package]\nversion = "${version}"\n`;
  const lock = (version) => `[[package]]\nname = "fm-semantic-worker"\nversion = "${version}"\n`;
  git('init', '-q', '-b', 'main');
  git('config', 'user.email', 'test@example.invalid');
  git('config', 'user.name', 'Test');
  const candidate = commit('worker candidate', {
    'Cargo.toml': cargo('0.7.0-1'),
    'Cargo.lock': lock('0.7.0-1'),
    'rust-toolchain.toml': '[toolchain]\nchannel = "1.98.1"\n',
    'crates/fm-semantic-worker/src/lib.rs': 'pub fn worker() {}\n',
  });
  const manifest = prepareRiskAcceptedGemmaManifest(
    Buffer.from(
      JSON.stringify({
        schemaVersion: 1,
        decision: 'noGo',
        repository: current.repository,
        releaseTag: 'semantic-v0.7.8',
        qualificationRunId: 42,
        sourceRevision: candidate,
        releaseCandidateFingerprint: 'sha256:abc',
        targets: current.targets,
      }),
    ),
    {
      modelRevision: current.riskAcceptedGemma.modelRevision,
      approvedBy: 'owner',
      approvedAt: '2026-11-16T12:00:00Z',
    },
  );
  return { root, git, commit, cargo, lock, candidate, manifest, write };
}

function environment(repo, overrides = {}) {
  return {
    root: repo.root,
    tag: `v${workspaceVersion(fs.readFileSync(join(repo.root, 'Cargo.toml'), 'utf8'))}`,
    revision: repo.git('rev-parse', 'HEAD'),
    vars: goodVars,
    mode: undefined,
    existingTags: [],
    onOriginMain: true,
    dirty: false,
    readManifest: (mode) => (mode === 'risk-accepted' ? repo.manifest : undefined),
    qualificationRuns: [],
    isAncestor: (a, b) => {
      try {
        repo.git('merge-base', '--is-ancestor', a, b);
        return true;
      } catch {
        return false;
      }
    },
    publishedTags: ['semantic-v0.7.8'],
    ...overrides,
  };
}

const failing = (report) => report.checks.filter((check) => !check.ok).map((check) => check.id);

test('passes when only a version bump and the manifest commit follow the signed candidate', (context) => {
  const repo = fixture(context);
  repo.commit('record manifest', { 'docs/m.json': '{}\n' });
  repo.commit('bump', { 'Cargo.toml': repo.cargo('0.7.0-2') });
  const report = runPreflight(environment(repo));
  assert.deepEqual(failing(report), []);
  assert.equal(report.ok, true);
  assert.match(formatReport(report), /Preflight passed/u);
});

test('names the changed worker input and the one-command fix when the candidate is stale', (context) => {
  const repo = fixture(context);
  repo.commit('bump', { 'Cargo.toml': repo.cargo('0.7.0-2') });
  repo.commit('worker change', { 'crates/fm-semantic-worker/src/lib.rs': 'pub fn x() {}\n' });
  const report = runPreflight(environment(repo));
  assert.deepEqual(failing(report), ['gemma-risk-accepted']);
  const text = formatReport(report);
  assert.match(text, /crates\/fm-semantic-worker\/src\/lib\.rs/u);
  assert.match(text, /fix: node scripts\/resign-gemma-candidate\.mjs --approve-risk-accepted /u);
  assert.match(text, /Preflight failed/u);
});

test('points at an automatic qualification run that already covers the release source', (context) => {
  const repo = fixture(context);
  const qualified = repo.commit('worker change', {
    'crates/fm-semantic-worker/src/lib.rs': 'pub fn x() {}\n',
  });
  repo.commit('bump', { 'Cargo.toml': repo.cargo('0.7.0-2') });
  const run = (databaseId, extra) => ({
    databaseId,
    headSha: qualified,
    createdAt: '2026-11-16T10:00:00Z',
    event: 'push',
    status: 'completed',
    conclusion: 'success',
    ...extra,
  });
  const report = runPreflight(
    environment(repo, {
      qualificationRuns: [
        run(1, { event: 'workflow_dispatch' }),
        run(2, { conclusion: 'failure' }),
        run(3),
      ],
    }),
  );
  const fix = report.checks.find((check) => check.id === 'gemma-risk-accepted').fix.join('\n');
  assert.match(fix, /--qualification-run 3\b/u);
});

test('does not reuse a qualification run whose commit predates a later worker change', (context) => {
  const repo = fixture(context);
  const early = repo.commit('worker change', {
    'crates/fm-semantic-worker/src/lib.rs': 'pub fn x() {}\n',
  });
  repo.commit('later worker change', {
    'crates/fm-semantic-worker/src/lib.rs': 'pub fn y() {}\n',
  });
  const report = runPreflight(
    environment(repo, {
      qualificationRuns: [
        {
          databaseId: 9,
          headSha: early,
          createdAt: '2026-11-16T10:00:00Z',
          event: 'push',
          status: 'completed',
          conclusion: 'success',
        },
      ],
    }),
  );
  const fix = report.checks.find((check) => check.id === 'gemma-risk-accepted').fix.join('\n');
  assert.doesNotMatch(fix, /--qualification-run/u);
});

test('reports a run still in progress instead of suggesting a new one', () => {
  const runs = classifyQualificationRuns(
    [
      {
        databaseId: 7,
        headSha: 'a',
        createdAt: '2026-11-16T10:00:00Z',
        event: 'push',
        status: 'in_progress',
        conclusion: '',
      },
    ],
    'b',
    { isAncestor: () => true, changesBetween: () => [] },
  );
  assert.equal(runs.reusable, undefined);
  assert.equal(runs.pending?.databaseId, 7);
  assert.match(remedies({ mode: 'risk-accepted', kind: 'worker-changed', runs })[0], /wait for/u);
});

test('rejects a tag that differs from the version, exists, or is not on main', (context) => {
  const repo = fixture(context);
  const report = runPreflight(
    environment(repo, { tag: 'v9.9.9', existingTags: ['v9.9.9'], onOriginMain: false }),
  );
  assert.deepEqual(failing(report).sort(), ['on-main', 'tag-free', 'tag-version']);
});

test('refuses combined Gemma modes and a missing qualification variable', (context) => {
  const repo = fixture(context);
  const combined = runPreflight(
    environment(repo, {
      vars: { ...goodVars, SEMANTIC_GEMMA_DESKTOP_EXPERIMENTAL: 'true' },
    }),
  );
  assert.ok(failing(combined).includes('modes'));
  const unqualified = runPreflight(
    environment(repo, { vars: { SEMANTIC_GEMMA_RISK_ACCEPTED: 'true' } }),
  );
  assert.ok(failing(unqualified).includes('variables'));
});

test('warns, rather than guesses, when GitHub variables cannot be read', (context) => {
  const repo = fixture(context);
  const report = runPreflight(environment(repo, { vars: undefined }));
  assert.equal(report.ok, true);
  assert.ok(report.checks.some((check) => check.level === 'warn' && check.id === 'variables'));
});

test('fails an active mode whose manifest is missing', (context) => {
  const repo = fixture(context);
  const report = runPreflight(
    environment(repo, {
      vars: {
        ...goodVars,
        SEMANTIC_GEMMA_RISK_ACCEPTED: 'false',
        SEMANTIC_GEMMA_DESKTOP_EXPERIMENTAL: 'true',
      },
    }),
  );
  assert.deepEqual(failing(report), ['gemma-experimental']);
});

test('parses the workspace version, the active modes and gate failures', () => {
  assert.equal(
    workspaceVersion(
      '[workspace]\nmembers = []\n\n[workspace.package]\nedition = "2024"\nversion = "1.2.3-4"\n\n[x]\nversion = "9"\n',
    ),
    '1.2.3-4',
  );
  assert.throws(() => workspaceVersion('[workspace]\n'), /not found/u);
  assert.deepEqual(activeGemmaModes({ SEMANTIC_GEMMA_RISK_ACCEPTED: 'true' }), ['risk-accepted']);
  assert.deepEqual(activeGemmaModes(undefined), []);
  assert.equal(
    classifyGateFailure('semantic worker build inputs changed after signed candidate:\nx'),
    'worker-changed',
  );
  assert.equal(
    classifyGateFailure('signed Gemma candidate source is not an ancestor of the desktop release'),
    'not-ancestor',
  );
  assert.equal(
    classifyGateFailure(
      'risk-accepted Gemma desktop requires independent semantic and owner approvals',
    ),
    'variables',
  );
  assert.equal(classifyGateFailure('anything else'), 'manifest');
  assert.match(remedies({ mode: 'experimental', kind: 'worker-changed' })[0], /no mechanical/u);
});

test('main-push planning qualifies exactly when the desktop gate would fail', (context) => {
  const repo = fixture(context);
  const changesBetween = (from, to) => semanticWorkerChanges(from, to, repo.root);
  const head = repo.commit('docs only', { 'docs/a.md': 'x\n' });
  const idle = planSemanticQualification({
    manifest: repo.manifest,
    revision: head,
    runNumber: 5,
    changesBetween,
  });
  assert.equal(idle.qualify, false);
  assert.equal(idle.releaseTag, '');
  const changed = repo.commit('worker', {
    'crates/fm-semantic-worker/src/lib.rs': 'pub fn z() {}\n',
  });
  const busy = planSemanticQualification({
    manifest: repo.manifest,
    revision: changed,
    runNumber: 5,
    changesBetween,
  });
  assert.equal(busy.qualify, true);
  assert.equal(busy.releaseTag, 'semantic-v0.7.9-auto.5');
});

test('qualification tags continue the numbered series and stay valid', () => {
  assert.equal(nextSemanticTag('semantic-v0.7.8-auto.57'), 'semantic-v0.7.9');
  assert.equal(autoQualificationTag('semantic-v0.7.8-auto.57', 58), 'semantic-v0.7.9-auto.58');
  assert.match(autoQualificationTag('semantic-v0.7.7', 1), /^semantic-v[0-9][0-9A-Za-z.-]*$/u);
  assert.throws(() => autoQualificationTag('semantic-v0.7.7', 0), /positive integer/u);
});

test('a resumed qualification dictates the tag; a fresh one takes the next number', () => {
  assert.equal(
    resolveReleaseTag({ candidateTag: 'semantic-v0.7.9-auto.5', currentTag: 'semantic-v0.7.8' }),
    'semantic-v0.7.9-auto.5',
  );
  assert.equal(
    resolveReleaseTag({ explicitTag: 'semantic-v1', currentTag: 'semantic-v0.7.8' }),
    'semantic-v1',
  );
  assert.equal(resolveReleaseTag({ currentTag: 'semantic-v0.7.8' }), 'semantic-v0.7.9');
  assert.throws(
    () =>
      resolveReleaseTag({
        explicitTag: 'semantic-v1',
        candidateTag: 'semantic-v2',
        currentTag: 'x1',
      }),
    /differs from the qualified candidate/u,
  );
});

test('the workflow proof rejects a push trigger that is not a plain main qualification', (context) => {
  const directory = fs.mkdtempSync(join(tmpdir(), 'push-trigger-'));
  context.after(() => fs.rmSync(directory, { force: true, recursive: true }));
  const original = fs.readFileSync('.github/workflows/release-semantic-components.yml', 'utf8');
  const mutate = (name, from, to) => {
    assert.ok(original.includes(from), `${name} anchor missing`);
    const file = join(directory, `${name}.yml`);
    fs.writeFileSync(file, original.replace(from, to));
    return file;
  };
  assert.doesNotThrow(() => checkSemanticQualificationWorkflow());
  assert.throws(
    () =>
      checkSemanticQualificationWorkflow(
        mutate('any-branch', 'branches: [main]', 'branches: ["**"]'),
      ),
    /qualification-only/u,
  );
  assert.throws(
    () =>
      checkSemanticQualificationWorkflow(
        mutate('unplanned', "needs.plan.outputs.qualify == 'true'", 'true'),
      ),
    /plan job/u,
  );
});
