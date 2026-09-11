import assert from 'node:assert/strict';
import fs from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { test } from 'node:test';

import {
  assertQualificationDispatchEnvironment,
  checkSemanticQualificationWorkflow,
} from './check-semantic-qualification-workflow.mjs';
import {
  nativeLibraryNames,
  parseNativeDependencies,
  recordAcceptedMacNotarization,
  verifyArchiveMembers,
  verifyNativeArchitecture,
  verifyPinnedFile,
  zvecRuntimeTarget,
} from './zvec-runtime-qualification.mjs';

test('pins every supported Zvec runtime to immutable upstream bytes', () => {
  const targets = [
    zvecRuntimeTarget('darwin', 'arm64'),
    zvecRuntimeTarget('win32', 'x64'),
    zvecRuntimeTarget('linux', 'x64'),
    zvecRuntimeTarget('linux', 'arm64'),
  ];
  assert.deepEqual(
    targets.map((target) => target.rustTarget),
    [
      'aarch64-apple-darwin',
      'x86_64-pc-windows-msvc',
      'x86_64-unknown-linux-gnu',
      'aarch64-unknown-linux-gnu',
    ],
  );
  for (const target of targets) {
    assert.match(target.archive.url, /releases\/download\/v0\.7\.0/u);
    assert.match(target.archive.sha256, /^[a-f0-9]{64}$/u);
    assert.match(target.loader.sha256, /^[a-f0-9]{64}$/u);
    assert.ok(target.archive.bytes > 0);
    assert.ok(target.loader.bytes > 0);
    assert.ok(target.expectedDependencies.length > 0);
  }
});

test('rejects unsupported targets and exposes only exact loader names', () => {
  assert.throws(() => zvecRuntimeTarget('darwin', 'x64'), /No production Zvec runtime/u);
  assert.throws(() => zvecRuntimeTarget('freebsd', 'x64'), /supported targets are/u);
  assert.deepEqual(nativeLibraryNames('darwin'), ['libzvec_c_api.dylib']);
  assert.deepEqual(nativeLibraryNames('win32'), ['zvec_c_api.dll']);
  assert.deepEqual(nativeLibraryNames('linux'), ['libzvec_c_api.so']);
});

test('rejects incomplete or expanded upstream runtime archives', () => {
  const linux = zvecRuntimeTarget('linux', 'x64');
  verifyArchiveMembers(['./TARGET', './libzvec_c_api.so'], linux);
  assert.throws(
    () => verifyArchiveMembers(['TARGET'], linux),
    /members are incomplete or unexpected/u,
  );
  assert.throws(
    () => verifyArchiveMembers(['TARGET', 'libzvec_c_api.so', 'companion.so'], linux),
    /members are incomplete or unexpected/u,
  );

  const windows = zvecRuntimeTarget('win32', 'x64');
  verifyArchiveMembers(['TARGET', 'zvec_c_api.dll', 'zvec_c_api.lib'], windows);
  assert.throws(
    () => verifyArchiveMembers(['TARGET', 'zvec_c_api.dll'], windows),
    /members are incomplete or unexpected/u,
  );
});

function machO(machine) {
  const bytes = Buffer.alloc(32);
  bytes.writeUInt32LE(0xfeedfacf, 0);
  bytes.writeUInt32LE(machine, 4);
  return bytes;
}

function elf(machine) {
  const bytes = Buffer.alloc(64);
  bytes.write('\x7fELF', 0, 'binary');
  bytes[4] = 2;
  bytes[5] = 1;
  bytes.writeUInt16LE(machine, 18);
  return bytes;
}

function pe(machine) {
  const bytes = Buffer.alloc(128);
  bytes.write('MZ', 0, 'ascii');
  bytes.writeUInt32LE(64, 0x3c);
  bytes.write('PE\0\0', 64, 'binary');
  bytes.writeUInt16LE(machine, 68);
  return bytes;
}

test('rejects wrong-architecture native loaders before packaging', () => {
  const macos = zvecRuntimeTarget('darwin', 'arm64');
  verifyNativeArchitecture(machO(0x0100000c), macos);
  assert.throws(
    () => verifyNativeArchitecture(machO(0x01000007), macos),
    /architecture does not match/u,
  );

  const linux = zvecRuntimeTarget('linux', 'x64');
  verifyNativeArchitecture(elf(62), linux);
  assert.throws(() => verifyNativeArchitecture(elf(183), linux), /architecture does not match/u);

  const windows = zvecRuntimeTarget('win32', 'x64');
  verifyNativeArchitecture(pe(0x8664), windows);
  assert.throws(
    () => verifyNativeArchitecture(pe(0xaa64), windows),
    /architecture does not match/u,
  );
});

test('parses platform dependency tools into deterministic dependency lists', () => {
  assert.deepEqual(
    parseNativeDependencies(
      'darwin',
      [
        'libzvec_c_api.dylib:',
        '\t/usr/lib/libSystem.B.dylib (compatibility version 1.0.0, current version 1336.0.0)',
        '\t/usr/lib/libc++.1.dylib (compatibility version 1.0.0, current version 1700.255.5)',
      ].join('\n'),
    ),
    ['/usr/lib/libSystem.B.dylib', '/usr/lib/libc++.1.dylib'],
  );
  assert.deepEqual(
    parseNativeDependencies(
      'linux',
      ' 0x0000000000000001 (NEEDED) Shared library: [libm.so.6]\n' +
        ' 0x0000000000000001 (NEEDED) Shared library: [libc.so.6]\n',
    ),
    ['libc.so.6', 'libm.so.6'],
  );
  assert.deepEqual(
    parseNativeDependencies('win32', 'Image has the following dependencies:\n KERNEL32.dll\n'),
    ['kernel32.dll'],
  );
});

test('rejects byte-length and digest drift in pinned files', () => {
  const directory = fs.mkdtempSync(path.join(tmpdir(), 'zvec-pinned-file-'));
  const file = path.join(directory, 'runtime');
  fs.writeFileSync(file, 'runtime');
  assert.throws(
    () => verifyPinnedFile(file, { bytes: 6, sha256: '0'.repeat(64) }, 'runtime'),
    /has 7 bytes/u,
  );
  assert.throws(
    () => verifyPinnedFile(file, { bytes: 7, sha256: '0'.repeat(64) }, 'runtime'),
    /hashed/u,
  );
  fs.rmSync(directory, { recursive: true, force: true });
});

test('release qualification dispatch is statically proven non-publishing', () => {
  assert.deepEqual(checkSemanticQualificationWorkflow(), {
    workflowDispatchCanPublish: false,
    tagSemanticPublicationRequiresQualification: true,
  });
});

test('private qualification rejects release events and enabled release gates', () => {
  assert.deepEqual(
    assertQualificationDispatchEnvironment({
      eventName: 'workflow_dispatch',
      semanticReleaseQualified: '',
      knowledgeSearchReleaseQualified: 'false',
    }),
    {
      workflowDispatchOnly: true,
      semanticReleaseQualified: false,
      knowledgeSearchReleaseQualified: false,
    },
  );
  assert.throws(
    () =>
      assertQualificationDispatchEnvironment({
        eventName: 'push',
        semanticReleaseQualified: '',
        knowledgeSearchReleaseQualified: '',
      }),
    /restricted to workflow_dispatch/u,
  );
  assert.throws(
    () =>
      assertQualificationDispatchEnvironment({
        eventName: 'workflow_dispatch',
        semanticReleaseQualified: 'true',
        knowledgeSearchReleaseQualified: '',
      }),
    /SEMANTIC_RELEASE_QUALIFIED must be absent or false/u,
  );
  assert.throws(
    () =>
      assertQualificationDispatchEnvironment({
        eventName: 'workflow_dispatch',
        semanticReleaseQualified: '',
        knowledgeSearchReleaseQualified: 'False',
      }),
    /KNOWLEDGE_SEARCH_RELEASE_QUALIFIED must be absent or false/u,
  );
});

test('release qualification proof rejects an unguarded release action', () => {
  const directory = fs.mkdtempSync(path.join(tmpdir(), 'unsafe-release-workflow-'));
  const workflow = path.join(directory, 'release.yml');
  fs.writeFileSync(
    workflow,
    [
      'jobs:',
      '  semantic-payloads:',
      "    if: github.event_name == 'workflow_dispatch' || vars.SEMANTIC_RELEASE_QUALIFIED == 'true'",
      '    steps:',
      '      - uses: softprops/action-gh-release@v2',
      '',
    ].join('\n'),
  );
  assert.throws(
    () => checkSemanticQualificationWorkflow(workflow),
    /can publish on workflow_dispatch/u,
  );
  fs.rmSync(directory, { recursive: true, force: true });
});

test('release qualification proof rejects write permission on a dispatch-reachable job', () => {
  const directory = fs.mkdtempSync(path.join(tmpdir(), 'unsafe-release-permissions-'));
  const workflow = path.join(directory, 'release.yml');
  fs.writeFileSync(
    workflow,
    [
      'permissions:',
      '  contents: write',
      'jobs:',
      '  qualification-safety:',
      "    if: github.event_name == 'workflow_dispatch'",
      '    steps: []',
      '',
    ].join('\n'),
  );
  assert.throws(
    () => checkSemanticQualificationWorkflow(workflow),
    /workflow_dispatch can receive contents: write/u,
  );
  fs.rmSync(directory, { recursive: true, force: true });
});

test('a bare accepted status cannot fabricate Apple notarization evidence', () => {
  const directory = fs.mkdtempSync(path.join(tmpdir(), 'notary-result-'));
  const result = path.join(directory, 'result.json');
  fs.writeFileSync(result, JSON.stringify({ status: 'Accepted' }));
  assert.throws(
    () => recordAcceptedMacNotarization('missing-bundle', result, 'missing-archive'),
    /accepted submission with an identifier/u,
  );
  fs.rmSync(directory, { recursive: true, force: true });
});
