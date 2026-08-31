// Validates the GitHub Actions CI workflow structurally, per TASKS/0004.
// Parses the YAML for real (rather than regex-matching text) so indentation
// mistakes that would break the workflow on GitHub fail this test too.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { load } from 'js-yaml';

const repoRoot = dirname(dirname(fileURLToPath(import.meta.url)));
const workflowPath = join(repoRoot, '.github', 'workflows', 'ci.yml');
const workflow = load(readFileSync(workflowPath, 'utf8'));

function allRunCommands(job) {
  return (job.steps ?? []).map((step) => step.run).filter((run) => typeof run === 'string');
}

test('workflow triggers on push and pull_request', () => {
  assert.ok('push' in workflow.on, 'expected an `on.push` trigger');
  assert.ok('pull_request' in workflow.on, 'expected an `on.pull_request` trigger');
});

test('rust job matrixes across ubuntu, macos and windows', () => {
  const rust = workflow.jobs.rust;
  assert.deepEqual([...rust.strategy.matrix.os].sort(), [
    'macos-latest',
    'ubuntu-latest',
    'windows-latest',
  ]);
  // biome-ignore lint/suspicious/noTemplateCurlyInString: GitHub Actions expression syntax.
  assert.equal(rust.runs_on ?? rust['runs-on'], '${{ matrix.os }}');
});

test('rust job runs fmt, clippy, nextest, and doctests across the workspace', () => {
  const commands = allRunCommands(workflow.jobs.rust);
  assert.ok(commands.some((c) => /cargo fmt --all --check/.test(c)));
  assert.ok(commands.some((c) => /cargo clippy --workspace --all-targets -- -D warnings/.test(c)));
  assert.ok(commands.some((c) => /cargo nextest run --workspace/.test(c)));
  assert.ok(commands.some((c) => /cargo test --doc --workspace/.test(c)));
});

test('rust job caches the cargo registry and target directory', () => {
  const uses = (workflow.jobs.rust.steps ?? []).map((step) => step.uses).filter(Boolean);
  assert.ok(
    uses.some((u) => /rust-cache/.test(u)),
    'expected a rust-cache action',
  );
});

test('frontend job runs on ubuntu-latest only (no OS matrix)', () => {
  const frontend = workflow.jobs.frontend;
  assert.equal(frontend['runs-on'], 'ubuntu-latest');
  assert.equal(frontend.strategy, undefined);
});

test('frontend job checks formatting, typechecks, tests and builds', () => {
  const commands = allRunCommands(workflow.jobs.frontend);
  assert.ok(
    commands.some((c) => /lint:frontend/.test(c)),
    'expected a biome/format check step',
  );
  assert.ok(
    commands.some((c) => /typecheck|tsc --noEmit/.test(c)),
    'expected a tsc --noEmit step',
  );
  assert.ok(
    commands.some((c) => /test:frontend/.test(c)),
    'expected a vitest step',
  );
  assert.ok(
    commands.some((c) => /build:frontend/.test(c)),
    'expected a production build step',
  );
});

test('frontend job caches the pnpm store', () => {
  const setupNode = (workflow.jobs.frontend.steps ?? []).find((step) =>
    /actions\/setup-node/.test(step.uses ?? ''),
  );
  assert.ok(setupNode, 'expected an actions/setup-node step');
  assert.equal(setupNode.with?.cache, 'pnpm');
});

test('audit job reports cargo audit and pnpm audit without blocking the workflow', () => {
  const audit = workflow.jobs.audit;
  assert.ok(audit, 'expected an `audit` job');
  const commands = allRunCommands(audit);
  assert.ok(commands.some((c) => /cargo audit/.test(c)));
  assert.ok(commands.some((c) => /pnpm audit/.test(c)));
  const jobLevelNonBlocking = audit['continue-on-error'] === true;
  const stepLevelNonBlocking = (audit.steps ?? [])
    .filter((step) => /cargo audit|pnpm audit/.test(step.run ?? ''))
    .every((step) => step['continue-on-error'] === true);
  assert.ok(
    jobLevelNonBlocking || stepLevelNonBlocking,
    'audit findings must not block the workflow',
  );
});

test('pull-request CI contains no code-signing or notarization steps', () => {
  const text = JSON.stringify(workflow).toLowerCase();
  assert.ok(!text.includes('codesign'));
  assert.ok(!text.includes('notariz'));
});

test('README documents CI with a badge and a short section', () => {
  const readme = readFileSync(join(repoRoot, 'README.md'), 'utf8');
  assert.match(readme, /badge\.svg/, 'expected a CI badge');
  assert.match(readme, /## CI/);
});
