import assert from 'node:assert/strict';
import fs from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { test } from 'node:test';

import {
  nativeLibraryNames,
  PRODUCTION_CHUNKER_IDENTITY,
  PRODUCTION_CONVERTER_IDENTITY,
  parseProductionBundleArguments,
  sourceBuildIdentity,
  supportedSemanticTarget,
} from './build-semantic-production-bundle.mjs';
import { modelDownloadRetryDelay } from './fetch-semantic-model.mjs';
import { resolveModelPack } from './verify-semantic-model.mjs';

test('model downloads honor bounded Retry-After delays', () => {
  const response = (retryAfter) => ({
    headers: new Headers(retryAfter ? { 'retry-after': retryAfter } : {}),
  });
  assert.equal(modelDownloadRetryDelay(response('7'), 0), 7_000);
  assert.equal(modelDownloadRetryDelay(response('120'), 0), 60_000);
  assert.equal(
    modelDownloadRetryDelay(response('Sat, 12 Sep 2026 16:00:09 GMT'), 0, 1_789_228_800_000),
    9_000,
  );
  assert.equal(modelDownloadRetryDelay(response(), 2), 8_000);
});

test('semantic production targets map only supported native platform pairs', () => {
  assert.deepEqual(supportedSemanticTarget('darwin', 'arm64'), {
    os: 'macos',
    arch: 'aarch64',
    rust: 'aarch64-apple-darwin',
  });
  assert.equal(supportedSemanticTarget('win32', 'x64').rust, 'x86_64-pc-windows-msvc');
  assert.equal(supportedSemanticTarget('linux', 'x64').rust, 'x86_64-unknown-linux-gnu');
  assert.equal(supportedSemanticTarget('linux', 'arm64').rust, 'aarch64-unknown-linux-gnu');
});

test('unsupported targets fail explicitly instead of receiving another target payload', () => {
  assert.throws(
    () => supportedSemanticTarget('darwin', 'x64'),
    /No production Zvec runtime is supported for darwin-x64/,
  );
  assert.throws(() => supportedSemanticTarget('freebsd', 'x64'), /supported targets are/);
});

test('production source identity requires a complete checked-out commit', () => {
  assert.throws(
    () => sourceBuildIdentity('v0.7.0', { requireClean: false }),
    /complete lowercase git commit SHA/u,
  );
  assert.throws(
    () => sourceBuildIdentity('0'.repeat(40), { requireClean: false }),
    /does not match checked-out HEAD/u,
  );
});

test('production bundle CLI accepts the package-manager argument separator', () => {
  const values = parseProductionBundleArguments([
    '--',
    '--output',
    'bundle',
    '--release-base-url',
    'https://qualification.invalid/revision',
    '--source-revision',
    'a'.repeat(40),
  ]);
  assert.equal(values.get('--output'), 'bundle');
  assert.equal(values.get('--release-base-url'), 'https://qualification.invalid/revision');
  assert.equal(values.get('--source-revision'), 'a'.repeat(40));
});

test('Zvec native library names are platform-specific', () => {
  assert.deepEqual(nativeLibraryNames('darwin'), ['libzvec_c_api.dylib']);
  assert.deepEqual(nativeLibraryNames('win32'), ['zvec_c_api.dll']);
  assert.deepEqual(nativeLibraryNames('linux'), ['libzvec_c_api.so']);
});

test('production bundle records the compiled structural chunker identity', () => {
  assert.equal(PRODUCTION_CHUNKER_IDENTITY, 'structural/3');
});

test('production bundle records the EPUB-capable baseline converter identity', () => {
  assert.equal(PRODUCTION_CONVERTER_IDENTITY, 'docling-pdf/1036000+baseline/2');
});

test('model verification resolves the content-addressed developer artifact', () => {
  const bundle = fs.mkdtempSync(path.join(tmpdir(), 'semantic-model-verification-'));
  const artifact = 'procyon.dev.model.multilingual-e5-small.v1.sha256.abc123';
  fs.mkdirSync(path.join(bundle, 'artifacts'));
  fs.writeFileSync(path.join(bundle, 'artifacts', artifact), 'model');
  fs.writeFileSync(
    path.join(bundle, 'catalog.json'),
    JSON.stringify({
      artifacts: [
        {
          id: artifact,
          component_id: 'procyon.dev.model.multilingual-e5-small',
        },
      ],
    }),
  );

  assert.equal(resolveModelPack(bundle), path.join(bundle, 'artifacts', artifact));
  fs.rmSync(bundle, { recursive: true, force: true });
});
