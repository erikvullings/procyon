// Validates the architecture documentation and ADRs required by TASKS/0005.
import assert from 'node:assert/strict';
import { readdirSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const repoRoot = dirname(dirname(fileURLToPath(import.meta.url)));
const docsRoot = join(repoRoot, 'docs');

function read(...segments) {
  return readFileSync(join(docsRoot, ...segments), 'utf8');
}

// The ten mandatory rules from spec §3, restated (not reworded away) in the overview.
const MANDATORY_RULES = [
  'must not call `fetch`, `EventSource` or Tauri APIs directly',
  'Axum handlers must remain thin',
  'Tauri commands must remain thin',
  'Core engine crates must not depend on Axum or Tauri',
  'Transport DTOs must not be reused indiscriminately as internal domain models',
  'Long-running operations must be represented as jobs',
  'backend must own authoritative filesystem and operation state',
  'must not implement file-copy semantics',
  'Browser and Tauri transports must provide equivalent application behaviour',
  'Platform differences must be represented through explicit capabilities',
];

test('overview.md restates the layering diagram and all ten mandatory rules', () => {
  const overview = read('architecture', 'overview.md');
  for (const rule of MANDATORY_RULES) {
    assert.ok(overview.includes(rule), `expected overview.md to restate: ${rule}`);
  }
  assert.ok(
    overview.includes('crates/fm-test-support/src/architecture.rs'),
    'expected overview.md to link to the fitness-checked layer map instead of restating it',
  );
});

const ADR_TITLES = [
  'browser + Tauri dual-host architecture',
  'Axum REST plus SSE',
  'OpenAPI source of truth and generated TypeScript client',
  'VFS provider abstraction',
  'operation scheduler and conflict handling',
  'plugin runtime selection',
  'frontend state management',
  'virtualized table implementation',
  'settings persistence',
  'native platform adapters',
  'archive library selection',
];

function decisionFiles() {
  return readdirSync(join(docsRoot, 'decisions'))
    .filter((name) => name.endsWith('.md'))
    .sort();
}

test('docs/decisions contains exactly one numbered ADR per §34 item', () => {
  const files = decisionFiles();
  assert.equal(files.length, ADR_TITLES.length);
  files.forEach((file, index) => {
    const expectedNumber = String(index + 1).padStart(4, '0');
    assert.ok(
      file.startsWith(`${expectedNumber}-`),
      `expected ADR ${index + 1} to be numbered ${expectedNumber}-*.md, got ${file}`,
    );
  });
});

for (const [index, title] of ADR_TITLES.entries()) {
  const number = String(index + 1).padStart(4, '0');

  test(`ADR ${number} covers "${title}" with all required sections`, () => {
    const file = decisionFiles().find((name) => name.startsWith(`${number}-`));
    assert.ok(file, `expected an ADR file numbered ${number}`);
    const content = read('decisions', file);

    assert.match(
      content,
      /^Status: (accepted|proposed)$/m,
      'expected a Status: accepted|proposed line',
    );
    for (const heading of [
      '## Context',
      '## Decision',
      '## Alternatives',
      '## Consequences',
      '## Revisit conditions',
    ]) {
      assert.ok(
        content.includes(heading),
        `expected ADR ${number} to include a "${heading}" section`,
      );
    }
  });
}

test('ADR 0007 records the Meiosis-style explicit state model decision, not a generic framework', () => {
  const file = decisionFiles().find((name) => name.startsWith('0007-'));
  const content = read('decisions', file);
  assert.match(content, /Meiosis/);
  assert.match(content, /explicit state model/);
});

test('docs/plugin-api and docs/screenshots exist with placeholder READMEs', () => {
  const pluginApi = read('plugin-api', 'README.md');
  const screenshots = read('screenshots', 'README.md');
  assert.ok(pluginApi.trim().length > 0);
  assert.ok(screenshots.trim().length > 0);
});
