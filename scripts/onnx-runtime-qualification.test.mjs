import assert from 'node:assert/strict';
import { test } from 'node:test';

import {
  ONNX_RUNTIME_SOURCE_REVISION,
  ONNX_RUNTIME_VERSION,
  onnxRuntimeTarget,
  verifyLinuxAbiCompatibility,
  verifyOnnxRuntimeArchiveMembers,
} from './onnx-runtime-qualification.mjs';

test('pins the Linux x86-64 ONNX Runtime to immutable Microsoft release bytes', () => {
  const target = onnxRuntimeTarget('linux', 'x64');
  assert.equal(ONNX_RUNTIME_VERSION, '1.28.0');
  assert.equal(ONNX_RUNTIME_SOURCE_REVISION, 'da9b5e364c465de65c49d91e696cd6485270757f');
  assert.equal(target.rustTarget, 'x86_64-unknown-linux-gnu');
  assert.equal(target.archive.assetId, 489174677);
  assert.equal(target.archive.bytes, 9_125_960);
  assert.equal(
    target.archive.sha256,
    'a3e1b79d7bb1bf09696ce675f49e4064e6c81f6202b8225624fff0e93f8d6407',
  );
  assert.equal(target.loader.sourceName, 'libonnxruntime.so.1.28.0');
  assert.equal(target.loader.name, 'libonnxruntime.so.1');
  assert.equal(target.loader.bytes, 24_268_848);
  assert.equal(
    target.loader.sha256,
    '1461ef7cc3d9e49982591721683cc3e3a55580aeca9a5254e7aac47b75ee4bab',
  );
});

test('rejects unsupported ONNX Runtime targets instead of using a host library', () => {
  assert.equal(onnxRuntimeTarget('darwin', 'arm64'), undefined);
  assert.equal(onnxRuntimeTarget('win32', 'x64'), undefined);
  assert.equal(onnxRuntimeTarget('linux', 'arm64'), undefined);
  assert.equal(onnxRuntimeTarget('linux', 'x64')?.loader.name, 'libonnxruntime.so.1');
});

test('rejects incomplete and expanded ONNX Runtime native packages', () => {
  const target = onnxRuntimeTarget('linux', 'x64');
  const members = [
    'onnxruntime-linux-x64-1.28.0/GIT_COMMIT_ID',
    'onnxruntime-linux-x64-1.28.0/VERSION_NUMBER',
    'onnxruntime-linux-x64-1.28.0/LICENSE',
    'onnxruntime-linux-x64-1.28.0/ThirdPartyNotices.txt',
    'onnxruntime-linux-x64-1.28.0/lib/libonnxruntime.so',
    'onnxruntime-linux-x64-1.28.0/lib/libonnxruntime.so.1',
    'onnxruntime-linux-x64-1.28.0/lib/libonnxruntime.so.1.28.0',
    'onnxruntime-linux-x64-1.28.0/lib/libonnxruntime_providers_shared.so',
  ];
  verifyOnnxRuntimeArchiveMembers(members, target);
  assert.throws(
    () =>
      verifyOnnxRuntimeArchiveMembers(
        members.filter((member) => !member.endsWith(target.loader.sourceName)),
        target,
      ),
    /native package is incomplete/u,
  );
  assert.throws(
    () =>
      verifyOnnxRuntimeArchiveMembers(
        [...members, 'onnxruntime-linux-x64-1.28.0/lib/libonnxruntime_providers_cuda.so'],
        target,
      ),
    /unexpected native library/u,
  );
});

const ubuntuCompatibleVersions = [
  '  004:   2 (GLIBC_2.2.5)   3 (GLIBCXX_3.4.21)   4 (CXXABI_1.3.11)',
  '  008:   5 (GLIBC_2.27)    2 (GLIBC_2.2.5)',
].join('\n');

test('accepts native ABI requirements within the Ubuntu 22.04 ceilings', () => {
  assert.deepEqual(verifyLinuxAbiCompatibility(ubuntuCompatibleVersions, 'OrtGetApiBase'), {
    glibc: '2.27',
    glibcxx: '3.4.21',
    cxxabi: '1.3.11',
  });
});

test('rejects the exact pyke static-library ABI failure pattern', () => {
  assert.throws(
    () =>
      verifyLinuxAbiCompatibility(`${ubuntuCompatibleVersions}\nGLIBC_2.38`, 'U __isoc23_strtol'),
    /requires GLIBC_2\.38; Ubuntu 22\.04 supports at most GLIBC_2\.35/u,
  );
  assert.throws(
    () =>
      verifyLinuxAbiCompatibility(
        `${ubuntuCompatibleVersions}\nGLIBCXX_3.4.32`,
        'U std::__cxx11::basic_string::_M_replace_cold',
      ),
    /requires GLIBCXX_3\.4\.32; Ubuntu 22\.04 supports at most GLIBCXX_3\.4\.30/u,
  );
  assert.throws(
    () =>
      verifyLinuxAbiCompatibility(`${ubuntuCompatibleVersions}\nCXXABI_1.3.14`, 'OrtGetApiBase'),
    /requires CXXABI_1\.3\.14; Ubuntu 22\.04 supports at most CXXABI_1\.3\.13/u,
  );
  assert.throws(
    () => verifyLinuxAbiCompatibility(ubuntuCompatibleVersions, 'U __isoc23_strtoull'),
    /forbidden Ubuntu 22\.04 symbol __isoc23_strtoull/u,
  );
});
