import assert from 'node:assert/strict';
import { test } from 'node:test';

import {
  ONNX_RUNTIME_SOURCE_REVISION,
  ONNX_RUNTIME_VERSION,
  onnxRuntimeTarget,
  verifyLinuxAbiCompatibility,
  verifyOnnxRuntimeArchiveMembers,
} from './onnx-runtime-qualification.mjs';

test('pins the Linux x86-64 ONNX Runtime to the immutable PyPI 1.28.0 wheel bytes', () => {
  const target = onnxRuntimeTarget('linux', 'x64');
  assert.equal(ONNX_RUNTIME_VERSION, '1.28.0');
  assert.equal(
    ONNX_RUNTIME_SOURCE_REVISION,
    'pypi-wheel-sha256-54fa221d669282bd8f582708ce4c96010a7e9fb0661f9006b37fe2fedafb73fe',
  );
  assert.equal(target.rustTarget, 'x86_64-unknown-linux-gnu');
  assert.equal(target.archive.bytes, 19_214_329);
  assert.equal(
    target.archive.sha256,
    '54fa221d669282bd8f582708ce4c96010a7e9fb0661f9006b37fe2fedafb73fe',
  );
  assert.equal(target.loader.sourceName, 'libonnxruntime.so.1.28.0');
  assert.equal(target.loader.name, 'libonnxruntime.so.1');
  assert.equal(target.loader.bytes, 24_272_944);
  assert.equal(
    target.loader.sha256,
    'aa4079d18f4ea7a5f3a94d80cd4bbe0f2740436626622d64d793803a20381083',
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
    'onnxruntime/LICENSE',
    'onnxruntime/ThirdPartyNotices.txt',
    'onnxruntime/capi/libonnxruntime.so.1.28.0',
    'onnxruntime/capi/libonnxruntime_providers_shared.so',
    'onnxruntime/capi/onnxruntime_pybind11_state.cpython-313-x86_64-linux-gnu.so',
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
        [...members, 'onnxruntime/capi/libonnxruntime_providers_cuda.so'],
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
