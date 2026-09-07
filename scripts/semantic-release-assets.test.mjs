import assert from 'node:assert/strict';
import fs from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { test } from 'node:test';

import { collectSemanticReleaseAssets } from './collect-semantic-release-assets.mjs';

function fixture() {
  return fs.mkdtempSync(path.join(tmpdir(), 'semantic-release-assets-'));
}

test('semantic release collection deduplicates identical model payloads and names target catalogs', () => {
  const root = fixture();
  const payloads = path.join(root, 'payloads');
  const catalogs = path.join(root, 'catalogs');
  const output = path.join(root, 'output');
  for (const target of ['linux-x86_64', 'windows-x86_64']) {
    fs.mkdirSync(path.join(payloads, `semantic-payloads-${target}`, 'artifacts'), {
      recursive: true,
    });
    fs.writeFileSync(
      path.join(payloads, `semantic-payloads-${target}`, 'artifacts', 'shared-model'),
      'same model',
    );
    fs.writeFileSync(
      path.join(payloads, `semantic-payloads-${target}`, 'artifacts', `worker-${target}`),
      target,
    );
    fs.mkdirSync(path.join(catalogs, `semantic-catalog-${target}`), { recursive: true });
    fs.writeFileSync(path.join(catalogs, `semantic-catalog-${target}`, 'catalog.json'), target);
    fs.writeFileSync(path.join(catalogs, `semantic-catalog-${target}`, 'catalog.sig'), target);
  }

  collectSemanticReleaseAssets(payloads, catalogs, output);

  assert.deepEqual(fs.readdirSync(output).sort(), [
    'semantic-catalog-linux-x86_64.json',
    'semantic-catalog-linux-x86_64.sig',
    'semantic-catalog-windows-x86_64.json',
    'semantic-catalog-windows-x86_64.sig',
    'shared-model',
    'worker-linux-x86_64',
    'worker-windows-x86_64',
  ]);
});

test('semantic release collection rejects conflicting bytes under one immutable ID', () => {
  const root = fixture();
  const payloads = path.join(root, 'payloads');
  const catalogs = path.join(root, 'catalogs');
  for (const [target, bytes] of [
    ['linux-x86_64', 'first'],
    ['windows-x86_64', 'second'],
  ]) {
    fs.mkdirSync(path.join(payloads, `semantic-payloads-${target}`, 'artifacts'), {
      recursive: true,
    });
    fs.writeFileSync(
      path.join(payloads, `semantic-payloads-${target}`, 'artifacts', 'same-id'),
      bytes,
    );
  }
  fs.mkdirSync(catalogs);

  assert.throws(
    () => collectSemanticReleaseAssets(payloads, catalogs, path.join(root, 'output')),
    /same-id has conflicting bytes/,
  );
});
