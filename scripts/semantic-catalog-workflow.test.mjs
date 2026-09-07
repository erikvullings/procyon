import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { load } from 'js-yaml';

const repoRoot = dirname(dirname(fileURLToPath(import.meta.url)));
const workflowPath = join(repoRoot, '.github', 'workflows', 'sign-semantic-catalog.yml');
const workflowText = readFileSync(workflowPath, 'utf8');
const workflow = load(workflowText);
const signJob = workflow.jobs.sign;

test('semantic catalog signing uses the protected desktop release environment', () => {
  assert.equal(signJob.environment, 'desktop-release');
  assert.equal(workflow.permissions.contents, 'read');
  assert.ok(workflow.on.workflow_call);
});

test('semantic catalog signing keeps the private key out of command arguments and outputs', () => {
  const commands = signJob.steps
    .map((step) => step.run)
    .filter((command) => typeof command === 'string')
    .join('\n');
  assert.match(commands, /RUNNER_TEMP\/semantic-catalog\.key/);
  assert.match(commands, /chmod 600/);
  assert.match(commands, /if: always|rm -f/);
  assert.doesNotMatch(commands, /\$\{\{\s*secrets\./);
  assert.doesNotMatch(workflowText, /echo\s+["']?\$SIGNING_KEY_BASE64/);
});

test('semantic catalog release job signs then verifies the exact payload set', () => {
  const commands = signJob.steps
    .map((step) => step.run)
    .filter((command) => typeof command === 'string');
  assert.ok(commands.some((command) => command.includes(' sign ')));
  assert.ok(commands.some((command) => command.includes(' verify ')));
  assert.ok(
    signJob.steps.some((step) => step.uses === 'actions/upload-artifact@v4'),
    'signed catalog must be retained as a workflow artifact',
  );
});
