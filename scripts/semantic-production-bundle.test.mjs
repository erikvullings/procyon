import assert from 'node:assert/strict';
import { test } from 'node:test';

import {
  nativeLibraryNames,
  PRODUCTION_CHUNKER_IDENTITY,
  PRODUCTION_CONVERTER_IDENTITY,
  sourceBuildIdentity,
  supportedSemanticTarget,
} from './build-semantic-production-bundle.mjs';

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
