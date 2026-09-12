import assert from 'node:assert/strict';
import fs from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { test } from 'node:test';

import {
  scanSemanticPrivacyEvidence,
  verifySemanticPrivacyEvidence,
} from './semantic-privacy-scan.mjs';

function fixture() {
  return fs.mkdtempSync(path.join(tmpdir(), 'semantic-privacy-scan-'));
}

function canary(category, value) {
  return { category, value };
}

test('privacy scan detects encoded canaries and values split across read boundaries', async () => {
  const root = fixture();
  const canaries = [
    canary('query', 'query canary 61b9d4f0'),
    canary('filename-path', '/private/canary/folder/report 86e77d42.txt'),
    canary('credential', 'credential-canary-4a90573f'),
    canary('token', 'token-canary-b5c1d99e'),
  ];
  fs.writeFileSync(path.join(root, 'application.log'), `${'x'.repeat(15)}${canaries[0].value}`);
  fs.writeFileSync(path.join(root, 'worker.bin'), Buffer.from(canaries[2].value, 'utf16le'));
  fs.writeFileSync(
    path.join(root, 'installer.log'),
    encodeURIComponent(canaries[1].value.replaceAll('/', '\\')),
  );
  fs.writeFileSync(
    path.join(root, 'crash.dmp'),
    Buffer.from(canaries[3].value).toString('base64url'),
  );

  const report = await scanSemanticPrivacyEvidence({
    root,
    canaries,
    chunkBytes: 16,
    now: new Date('2026-09-10T12:00:00Z'),
  });

  assert.equal(report.status, 'fail');
  assert.deepEqual([...new Set(report.findings.map(({ category }) => category))].sort(), [
    'credential',
    'filename-path',
    'query',
    'token',
  ]);
  const serialized = JSON.stringify(report);
  for (const entry of canaries) {
    assert.doesNotMatch(
      serialized,
      new RegExp(entry.value.replaceAll(/[.*+?^${}()|[\]\\]/g, '\\$&')),
    );
  }
  assert.equal(fs.existsSync(path.join(root, 'application.log')), false);
  assert.ok(report.findings.every(({ removed }) => removed));
});

test('privacy scan removes a previewed scoped capture and retains no sensitive values', async () => {
  const root = fixture();
  const capture = path.join(root, 'diagnostics', 'authorized.capture');
  fs.mkdirSync(path.dirname(capture), { recursive: true });
  const canaries = [
    canary('prompt', 'prompt-canary-c1c2c3c4'),
    canary('response', 'response-canary-d4d3d2d1'),
  ];
  fs.writeFileSync(capture, `${canaries[0].value}\n${canaries[1].value}\n`);

  const report = await scanSemanticPrivacyEvidence({
    root,
    canaries,
    captures: [
      {
        path: 'diagnostics/authorized.capture',
        categories: ['prompt', 'response'],
        previewedAt: '2026-09-10T11:55:00Z',
        expiresAt: '2026-09-10T12:05:00Z',
      },
    ],
    now: new Date('2026-09-10T12:00:00Z'),
  });

  assert.equal(report.status, 'pass');
  assert.equal(report.authorizedCaptures.length, 1);
  assert.equal(report.authorizedCaptures[0].removed, true);
  assert.equal(fs.existsSync(capture), false);
  assert.doesNotMatch(JSON.stringify(report), /prompt-canary|response-canary/);
});

test('privacy scan fails closed for expired or over-broad diagnostic capture', async () => {
  const root = fixture();
  const capture = path.join(root, 'capture.bin');
  const canaries = [
    canary('authorization-header', 'Bearer authorization-canary-00d5f128'),
    canary('model-payload', 'model-payload-canary-f1274f23'),
  ];
  fs.writeFileSync(capture, Buffer.from(canaries[0].value, 'utf16le'));

  const report = await scanSemanticPrivacyEvidence({
    root,
    canaries,
    captures: [
      {
        path: 'capture.bin',
        categories: ['model-payload'],
        previewedAt: '2026-09-10T11:00:00Z',
        expiresAt: '2026-09-10T11:15:00Z',
      },
    ],
    now: new Date('2026-09-10T12:00:00Z'),
  });

  assert.equal(report.status, 'fail');
  assert.equal(report.findings[0].category, 'authorization-header');
  assert.equal(fs.existsSync(capture), false);
});

test('privacy evidence verification rejects changed, missing, and added files', async () => {
  const root = fixture();
  fs.writeFileSync(path.join(root, 'application.log'), 'safe application output');
  const report = await scanSemanticPrivacyEvidence({
    root,
    canaries: [canary('query', 'query-canary-3f024412')],
    now: new Date('2026-09-10T12:00:00Z'),
  });
  assert.equal(report.status, 'pass');
  await verifySemanticPrivacyEvidence({ root, report });

  fs.appendFileSync(path.join(root, 'application.log'), '\ntampered');
  await assert.rejects(verifySemanticPrivacyEvidence({ root, report }), /evidence digest changed/u);

  fs.rmSync(path.join(root, 'application.log'));
  await assert.rejects(
    verifySemanticPrivacyEvidence({ root, report }),
    /evidence file is missing/u,
  );

  fs.writeFileSync(path.join(root, 'application.log'), 'safe application output');
  fs.writeFileSync(path.join(root, 'unexpected.log'), 'new evidence');
  await assert.rejects(
    verifySemanticPrivacyEvidence({ root, report }),
    /unexpected evidence file/u,
  );
});

test('privacy scan rejects unsafe canary metadata and evidence symlinks', async () => {
  const root = fixture();
  await assert.rejects(
    scanSemanticPrivacyEvidence({
      root,
      canaries: [canary('../query', 'query-canary-f1291aa2')],
    }),
    /invalid canary category/u,
  );

  const target = path.join(root, 'target.log');
  fs.writeFileSync(target, 'safe');
  const link = path.join(root, 'linked.log');
  try {
    fs.symlinkSync(target, link);
  } catch (error) {
    if (process.platform === 'win32' && error.code === 'EPERM') return;
    throw error;
  }
  await assert.rejects(
    scanSemanticPrivacyEvidence({
      root,
      canaries: [canary('query', 'query-canary-f1291aa2')],
    }),
    /symbolic link/u,
  );
});
