import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import fs from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const repositoryRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

export const ZVEC_VERSION = '0.7.0';
export const ZVEC_RUST_REVISION = '733e0bc82e02a0c63202bff594a7f4530520dfd0';
export const ZVEC_NATIVE_REVISION = '8321c1314a559fd5f909e92498f43e5194bf9b99';

const rustApacheLicense = {
  url: `https://raw.githubusercontent.com/zvec-ai/zvec-rust/${ZVEC_RUST_REVISION}/LICENSE`,
  bytes: 11356,
  sha256: '43070e2d4e532684de521b885f385d0841030efa2b1a20bafb76133a5e1379c1',
};

const nativeApacheLicense = {
  url: `https://raw.githubusercontent.com/alibaba/zvec/${ZVEC_NATIVE_REVISION}/LICENSE`,
  bytes: 11356,
  sha256: '43070e2d4e532684de521b885f385d0841030efa2b1a20bafb76133a5e1379c1',
};

const nativeNotice = {
  url: `https://raw.githubusercontent.com/alibaba/zvec/${ZVEC_NATIVE_REVISION}/NOTICE`,
  bytes: 5020,
  sha256: '332b1a498b446fab1232b671c2ba74102fc563c198dc6f53980d1282075958ad',
};

const releaseBase = `https://github.com/zvec-ai/zvec-rust/releases/download/v${ZVEC_VERSION}`;

const targetDescriptors = new Map([
  [
    'darwin-arm64',
    {
      platform: 'darwin',
      processArchitecture: 'arm64',
      operatingSystem: 'macos',
      architecture: 'aarch64',
      rustTarget: 'aarch64-apple-darwin',
      archive: {
        assetId: 530247359,
        name: 'zvec-prebuilt-aarch64-apple-darwin.tar.gz',
        bytes: 8135328,
        sha256: '59c41dcbaab69b9fbcf3ca0f1997f58f189a025657fd09a464dca199107cdeb2',
      },
      loader: {
        name: 'libzvec_c_api.dylib',
        bytes: 23146352,
        sha256: 'c9e4bf9387ef7261a284de407ec7e48ac9a48309d8daaa4c5ed85a8fa5bb4763',
        format: 'mach-o',
        machine: 0x0100000c,
      },
      expectedDependencies: [
        '/System/Library/Frameworks/CoreFoundation.framework/Versions/A/CoreFoundation',
        '/usr/lib/libSystem.B.dylib',
        '/usr/lib/libc++.1.dylib',
      ],
    },
  ],
  [
    'win32-x64',
    {
      platform: 'win32',
      processArchitecture: 'x64',
      operatingSystem: 'windows',
      architecture: 'x86_64',
      rustTarget: 'x86_64-pc-windows-msvc',
      archive: {
        assetId: 530247358,
        name: 'zvec-prebuilt-x86_64-pc-windows-msvc.tar.gz',
        bytes: 8839865,
        sha256: 'd8fe5585ad83066038f6e60990fe6e69528637a58fffc5024ca211c187a9d49a',
      },
      loader: {
        name: 'zvec_c_api.dll',
        bytes: 26570240,
        sha256: '3745106b3beee6be2d50ca678b46d3f0289afb5136ff51e1d1ec037a27b29e4a',
        format: 'pe',
        machine: 0x8664,
      },
      linkLibrary: {
        name: 'zvec_c_api.lib',
        bytes: 110340,
        sha256: '404d08fc55680a1bbc4351041826d5b643ebeb1767ad19931cb9e077aa24f7f7',
      },
      expectedDependencies: [
        'dbghelp.dll',
        'kernel32.dll',
        'ole32.dll',
        'rpcrt4.dll',
        'shell32.dll',
        'shlwapi.dll',
      ],
    },
  ],
  [
    'linux-x64',
    {
      platform: 'linux',
      processArchitecture: 'x64',
      operatingSystem: 'linux',
      architecture: 'x86_64',
      rustTarget: 'x86_64-unknown-linux-gnu',
      archive: {
        assetId: 530247354,
        name: 'zvec-prebuilt-x86_64-unknown-linux-gnu.tar.gz',
        bytes: 13331422,
        sha256: '7e9adbeadc42c772665efed45112220aa895d3f7963fa03c016102f2f414c37f',
      },
      loader: {
        name: 'libzvec_c_api.so',
        bytes: 36854864,
        sha256: '89eac719eb426a2066d2104e5b1199aa83ec18eaa4c31c7797b9bf469904cfd5',
        format: 'elf',
        machine: 62,
      },
      expectedDependencies: [
        'ld-linux-x86-64.so.2',
        'libc.so.6',
        'libdl.so.2',
        'libm.so.6',
        'libpthread.so.0',
        'librt.so.1',
      ],
    },
  ],
  [
    'linux-arm64',
    {
      platform: 'linux',
      processArchitecture: 'arm64',
      operatingSystem: 'linux',
      architecture: 'aarch64',
      rustTarget: 'aarch64-unknown-linux-gnu',
      archive: {
        assetId: 530247355,
        name: 'zvec-prebuilt-aarch64-unknown-linux-gnu.tar.gz',
        bytes: 11784274,
        sha256: '0195a85f07370d7bcbf26f990bf794e31430e00224c8b9303d43ea677db6f77d',
      },
      loader: {
        name: 'libzvec_c_api.so',
        bytes: 32470624,
        sha256: '621af6ba8249ce44dc17fb05da6c51c723cc466843e7f46ee44a40bd7eee1169',
        format: 'elf',
        machine: 183,
      },
      expectedDependencies: ['ld-linux-aarch64.so.1', 'libc.so.6', 'libm.so.6'],
    },
  ],
]);

for (const descriptor of targetDescriptors.values()) {
  descriptor.archive.url = `${releaseBase}/${descriptor.archive.name}`;
}

function supportedTargetDescription() {
  return 'macOS arm64, Windows x64, Linux x64, and Linux arm64';
}

export function zvecRuntimeTarget(platform = process.platform, processArchitecture = process.arch) {
  const descriptor = targetDescriptors.get(`${platform}-${processArchitecture}`);
  if (!descriptor) {
    throw new Error(
      `No production Zvec runtime is supported for ${platform}-${processArchitecture}; ` +
        `supported targets are ${supportedTargetDescription()}.`,
    );
  }
  return descriptor;
}

export function nativeLibraryNames(platform = process.platform) {
  if (platform === 'darwin') return ['libzvec_c_api.dylib'];
  if (platform === 'win32') return ['zvec_c_api.dll'];
  return ['libzvec_c_api.so'];
}

export function sha256Of(file) {
  const hash = createHash('sha256');
  const handle = fs.openSync(file, 'r');
  try {
    const buffer = Buffer.allocUnsafe(4 * 1024 * 1024);
    for (;;) {
      const read = fs.readSync(handle, buffer, 0, buffer.length, null);
      if (read === 0) break;
      hash.update(buffer.subarray(0, read));
    }
  } finally {
    fs.closeSync(handle);
  }
  return hash.digest('hex');
}

export function verifyPinnedFile(file, expected, label) {
  if (!fs.existsSync(file) || !fs.statSync(file).isFile()) {
    throw new Error(`${label} is missing`);
  }
  const bytes = fs.statSync(file).size;
  if (bytes !== expected.bytes) {
    throw new Error(`${label} has ${bytes} bytes; expected ${expected.bytes}`);
  }
  const sha256 = sha256Of(file);
  if (sha256 !== expected.sha256) {
    throw new Error(`${label} hashed ${sha256}; expected ${expected.sha256}`);
  }
}

function normalizedArchiveMember(member) {
  return member
    .replaceAll('\\', '/')
    .replace(/^\.\/+/, '')
    .replace(/\/$/, '');
}

function expectedArchiveMembers(descriptor) {
  return [
    'TARGET',
    descriptor.loader.name,
    ...(descriptor.linkLibrary ? [descriptor.linkLibrary.name] : []),
  ].sort();
}

export function verifyArchiveMembers(members, descriptor) {
  const actual = [...new Set(members.map(normalizedArchiveMember).filter(Boolean))].sort();
  const expected = expectedArchiveMembers(descriptor);
  if (JSON.stringify(actual) !== JSON.stringify(expected)) {
    throw new Error(
      `Zvec archive members are incomplete or unexpected: found ${actual.join(', ') || '(none)'}; ` +
        `expected ${expected.join(', ')}`,
    );
  }
}

function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    cwd: repositoryRoot,
    env: process.env,
    encoding: 'utf8',
    ...options,
  });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    throw new Error(
      `${command} ${args.join(' ')} exited with status ${result.status ?? 'unknown'}\n` +
        `${result.stdout ?? ''}${result.stderr ?? ''}`,
    );
  }
  return result;
}

export function verifyMacDeveloperIdSignature(file) {
  run('codesign', ['--verify', '--strict', '--verbose=2', file]);
  const result = run('codesign', ['-dv', '--verbose=4', file]);
  const details = `${result.stdout}${result.stderr}`;
  if (!details.includes('Authority=Developer ID Application')) {
    throw new Error('macOS semantic payload is not signed by a Developer ID Application identity');
  }
}

function archiveMembers(archive) {
  return run('tar', ['-tzf', archive]).stdout.split(/\r?\n/u);
}

export function verifyNativeArchitecture(bytes, descriptor) {
  let format;
  let machine;
  if (bytes.length >= 8 && bytes.readUInt32LE(0) === 0xfeedfacf) {
    format = 'mach-o';
    machine = bytes.readUInt32LE(4);
  } else if (
    bytes.length >= 20 &&
    bytes[0] === 0x7f &&
    bytes.subarray(1, 4).toString('ascii') === 'ELF'
  ) {
    if (bytes[4] !== 2 || bytes[5] !== 1) {
      throw new Error('Zvec ELF loader is not a little-endian 64-bit binary');
    }
    format = 'elf';
    machine = bytes.readUInt16LE(18);
  } else if (bytes.length >= 64 && bytes.subarray(0, 2).toString('ascii') === 'MZ') {
    const header = bytes.readUInt32LE(0x3c);
    if (
      header + 6 > bytes.length ||
      bytes.subarray(header, header + 4).toString('binary') !== 'PE\0\0'
    ) {
      throw new Error('Zvec Windows loader has an invalid PE header');
    }
    format = 'pe';
    machine = bytes.readUInt16LE(header + 4);
  } else {
    throw new Error('Zvec runtime has an unrecognized native binary format');
  }
  if (format !== descriptor.loader.format || machine !== descriptor.loader.machine) {
    throw new Error(
      `Zvec runtime architecture does not match ${descriptor.rustTarget}: ` +
        `found ${format} machine 0x${machine.toString(16)}`,
    );
  }
}

export function parseNativeDependencies(platform, output) {
  if (platform === 'darwin') {
    return output
      .split(/\r?\n/u)
      .slice(1)
      .map((line) => line.trim().replace(/ \(compatibility version.*$/u, ''))
      .filter(Boolean)
      .sort();
  }
  if (platform === 'win32') {
    return output
      .split(/\r?\n/u)
      .map((line) => line.trim().toLowerCase())
      .filter((line) => /^[a-z0-9_.-]+\.dll$/u.test(line))
      .sort();
  }
  return [...output.matchAll(/\(NEEDED\).*Shared library: \[([^\]]+)\]/gu)]
    .map((match) => match[1])
    .sort();
}

export function inspectNativeDependencies(file, descriptor) {
  let tool;
  let result;
  if (descriptor.platform === 'darwin') {
    tool = 'otool -L';
    result = run('otool', ['-L', file]);
  } else if (descriptor.platform === 'win32') {
    tool = 'dumpbin /dependents';
    result = run('dumpbin', ['/nologo', '/dependents', file]);
  } else {
    tool = 'readelf -d';
    result = run('readelf', ['-d', file]);
  }
  const dependencies = parseNativeDependencies(descriptor.platform, result.stdout).filter(
    (dependency) => dependency !== `@rpath/${descriptor.loader.name}`,
  );
  const expected = [...descriptor.expectedDependencies].sort();
  if (JSON.stringify(dependencies) !== JSON.stringify(expected)) {
    throw new Error(
      `Zvec runtime dependencies are incomplete or unexpected: found ` +
        `${dependencies.join(', ') || '(none)'}; expected ${expected.join(', ')}`,
    );
  }
  return { tool, dependencies };
}

export function verifyRuntimeBinary(file, descriptor, { pinned = false } = {}) {
  if (pinned) verifyPinnedFile(file, descriptor.loader, 'pinned Zvec native loader');
  verifyNativeArchitecture(fs.readFileSync(file), descriptor);
  return inspectNativeDependencies(file, descriptor);
}

async function downloadPinnedFile(url, destination, descriptor) {
  const partial = `${destination}.partial`;
  fs.rmSync(partial, { force: true });
  const response = await fetch(url, { redirect: 'follow' });
  if (!response.ok || !response.body) {
    throw new Error(`${url} returned HTTP ${response.status}`);
  }
  let handle;
  try {
    handle = await fs.promises.open(partial, 'w');
    for await (const chunk of response.body) {
      await handle.write(chunk);
    }
    await handle.close();
    handle = undefined;
    verifyPinnedFile(partial, descriptor, `downloaded ${descriptor.name}`);
    fs.renameSync(partial, destination);
  } catch (error) {
    await handle?.close();
    fs.rmSync(partial, { force: true });
    throw error;
  }
}

function verifyExtractedDirectory(directory, descriptor) {
  const members = fs.readdirSync(directory, { withFileTypes: true }).map((entry) => {
    if (!entry.isFile()) {
      throw new Error(`Zvec runtime package member ${entry.name} is not a regular file`);
    }
    return entry.name;
  });
  verifyArchiveMembers(members, descriptor);
  const target = fs.readFileSync(path.join(directory, 'TARGET'), 'utf8').trim();
  if (target !== descriptor.rustTarget) {
    throw new Error(`Zvec runtime TARGET is ${target}; expected ${descriptor.rustTarget}`);
  }
  const library = path.join(directory, descriptor.loader.name);
  verifyPinnedFile(library, descriptor.loader, 'pinned Zvec native loader');
  if (descriptor.linkLibrary) {
    verifyPinnedFile(
      path.join(directory, descriptor.linkLibrary.name),
      descriptor.linkLibrary,
      'pinned Zvec import library',
    );
  }
  const dependencyEvidence = verifyRuntimeBinary(library, descriptor);
  return { directory, library, dependencyEvidence };
}

export async function preparePinnedZvecRuntime(descriptor, cacheRoot) {
  const root = path.resolve(cacheRoot);
  const archiveDirectory = path.join(root, 'archives');
  const extractedDirectory = path.join(root, ZVEC_VERSION, descriptor.rustTarget);
  const archive = path.join(archiveDirectory, descriptor.archive.name);
  fs.mkdirSync(archiveDirectory, { recursive: true });

  if (fs.existsSync(archive)) {
    try {
      verifyPinnedFile(archive, descriptor.archive, 'cached Zvec release archive');
    } catch {
      fs.rmSync(archive, { force: true });
    }
  }
  if (!fs.existsSync(archive)) {
    await downloadPinnedFile(descriptor.archive.url, archive, descriptor.archive);
  }

  if (fs.existsSync(extractedDirectory)) {
    try {
      return verifyExtractedDirectory(extractedDirectory, descriptor);
    } catch {
      fs.rmSync(extractedDirectory, { recursive: true, force: true });
    }
  }

  const staging = `${extractedDirectory}.extracting`;
  fs.rmSync(staging, { recursive: true, force: true });
  fs.mkdirSync(staging, { recursive: true });
  try {
    verifyArchiveMembers(archiveMembers(archive), descriptor);
    run('tar', ['-xzf', archive, '-C', staging]);
    const verified = verifyExtractedDirectory(staging, descriptor);
    fs.mkdirSync(path.dirname(extractedDirectory), { recursive: true });
    fs.renameSync(staging, extractedDirectory);
    return {
      ...verified,
      directory: extractedDirectory,
      library: path.join(extractedDirectory, descriptor.loader.name),
    };
  } catch (error) {
    fs.rmSync(staging, { recursive: true, force: true });
    throw error;
  }
}

function catalogManifest(bundle) {
  return JSON.parse(fs.readFileSync(path.join(bundle, 'catalog-input.json'), 'utf8'));
}

function catalogRuntime(bundle) {
  const manifest = catalogManifest(bundle);
  const artifact = manifest.catalog.artifacts.find(
    (candidate) => candidate.component_id === 'procyon.semantic.zvec-runtime',
  );
  if (!artifact) throw new Error('production catalog has no Zvec runtime artifact');
  return artifact;
}

function artifactPath(bundle, artifact) {
  return path.join(bundle, 'artifacts', artifact.id);
}

function catalogSha256(value) {
  if (!Array.isArray(value) || value.length !== 32) {
    throw new Error('catalog SHA-256 does not contain 32 bytes');
  }
  return Buffer.from(value).toString('hex');
}

function notarizedArtifactEvidence(bundle, manifest) {
  const evidence = manifest.catalog.artifacts
    .filter(
      (artifact) =>
        artifact.component_id === 'procyon.semantic.worker' ||
        artifact.component_id === 'procyon.semantic.zvec-runtime',
    )
    .map((artifact) => {
      const file = artifactPath(bundle, artifact);
      const byteLength = fs.statSync(file).size;
      const sha256 = sha256Of(file);
      if (
        artifact.resources.download_bytes !== byteLength ||
        catalogSha256(artifact.checksum) !== sha256
      ) {
        throw new Error(`notarized artifact ${artifact.id} disagrees with the catalog`);
      }
      return { artifactId: artifact.id, byteLength, sha256 };
    })
    .sort((left, right) => left.artifactId.localeCompare(right.artifactId));
  if (evidence.length !== 2) {
    throw new Error('notarization requires exactly one worker and one Zvec runtime artifact');
  }
  return evidence;
}

function verifySubmittedNotarizationArchive(bundle, manifest, submittedArchive) {
  const expected = notarizedArtifactEvidence(bundle, manifest);
  const extracted = fs.mkdtempSync(path.join(tmpdir(), 'procyon-notarization-evidence-'));
  try {
    run('ditto', ['-x', '-k', submittedArchive, extracted]);
    const pending = [extracted];
    const files = [];
    while (pending.length > 0) {
      const directory = pending.pop();
      for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
        const entryPath = path.join(directory, entry.name);
        if (entry.isDirectory()) {
          pending.push(entryPath);
        } else if (entry.isFile()) {
          files.push(entryPath);
        } else {
          throw new Error('notarization archive contains a non-regular member');
        }
      }
    }
    const actual = files
      .map((file) => ({
        artifactId: path.basename(file),
        byteLength: fs.statSync(file).size,
        sha256: sha256Of(file),
      }))
      .sort((left, right) => left.artifactId.localeCompare(right.artifactId));
    if (!sameJson(actual, expected)) {
      throw new Error('notarization archive bytes do not match the packaged worker and runtime');
    }
    return expected;
  } finally {
    fs.rmSync(extracted, { recursive: true, force: true });
  }
}

function expectedUpstreamMetadata(descriptor) {
  return {
    zvecRust: {
      repository: 'https://github.com/zvec-ai/zvec-rust',
      revision: ZVEC_RUST_REVISION,
      tag: `v${ZVEC_VERSION}`,
      crate: {
        name: 'zvec-rust',
        version: ZVEC_VERSION,
        sha256: '09da6c0c29360b764d54f8d4107174f1fb60921d25387fd433f263ec4bf19e5a',
      },
      sysCrate: {
        name: 'zvec-rust-sys',
        version: ZVEC_VERSION,
        sha256: 'e1ea26d758a283798af569947fe9e3fb29e10cbffc5b09f50ef626cedfc4e013',
      },
    },
    nativeZvec: {
      repository: 'https://github.com/alibaba/zvec',
      revision: ZVEC_NATIVE_REVISION,
      tag: `v${ZVEC_VERSION}`,
    },
    releaseAsset: {
      assetId: descriptor.archive.assetId,
      name: descriptor.archive.name,
      url: descriptor.archive.url,
      byteLength: descriptor.archive.bytes,
      sha256: descriptor.archive.sha256,
      loaderByteLength: descriptor.loader.bytes,
      loaderSha256: descriptor.loader.sha256,
    },
  };
}

function redistributionMetadata(signingStatus) {
  return {
    spdx: 'Apache-2.0',
    upstreamSourceModified: false,
    artifactTransformation:
      signingStatus === 'developer-id-verified' ? 'developer-id-code-signature' : 'none',
    licenses: {
      zvecRust: rustApacheLicense,
      nativeZvec: nativeApacheLicense,
    },
    notice: nativeNotice,
    requiredAttributions: ['Unicode Character Database (Unicode-3.0)', 'pyglass (MIT)'],
  };
}

export function writeZvecRuntimeQualification({
  bundle,
  descriptor,
  procyonRevision,
  workingTreeStatus,
  dependencyEvidence,
  signingStatus,
  notarizationStatus,
}) {
  const runtime = catalogRuntime(bundle);
  const packedRuntime = artifactPath(bundle, runtime);
  const byteLength = fs.statSync(packedRuntime).size;
  const sha256 = sha256Of(packedRuntime);
  if (
    catalogSha256(runtime.checksum) !== sha256 ||
    runtime.resources.download_bytes !== byteLength
  ) {
    throw new Error('packed Zvec runtime bytes disagree with catalog integrity metadata');
  }
  const report = {
    formatVersion: 1,
    componentId: runtime.component_id,
    artifactId: runtime.id,
    zvecVersion: ZVEC_VERSION,
    target: {
      operatingSystem: descriptor.operatingSystem,
      architecture: descriptor.architecture,
      rustTarget: descriptor.rustTarget,
    },
    loader: {
      fileName: descriptor.loader.name,
      byteLength,
      sha256,
    },
    upstream: expectedUpstreamMetadata(descriptor),
    redistribution: redistributionMetadata(signingStatus),
    build: {
      source: 'https://github.com/erikvullings/procyon',
      revision: procyonRevision,
      workingTree: workingTreeStatus,
    },
    dynamicDependencies: dependencyEvidence,
    signing: { status: signingStatus },
    notarization: { status: notarizationStatus },
  };
  fs.writeFileSync(
    path.join(bundle, 'zvec-runtime-qualification.json'),
    `${JSON.stringify(report, null, 2)}\n`,
  );
  return report;
}

function sameJson(left, right) {
  return JSON.stringify(left) === JSON.stringify(right);
}

export function verifyZvecRuntimeQualification(bundle, { requireProductionTrust = false } = {}) {
  const reportPath = path.join(bundle, 'zvec-runtime-qualification.json');
  const report = JSON.parse(fs.readFileSync(reportPath, 'utf8'));
  const descriptor = [...targetDescriptors.values()].find(
    (candidate) => candidate.rustTarget === report.target?.rustTarget,
  );
  if (!descriptor) throw new Error('Zvec qualification report has an unsupported target');
  if (
    report.formatVersion !== 1 ||
    report.zvecVersion !== ZVEC_VERSION ||
    !sameJson(report.upstream, expectedUpstreamMetadata(descriptor)) ||
    !sameJson(report.redistribution, redistributionMetadata(report.signing?.status))
  ) {
    throw new Error('Zvec qualification report provenance does not match the pinned release');
  }
  if (
    report.target.operatingSystem !== descriptor.operatingSystem ||
    report.target.architecture !== descriptor.architecture ||
    report.loader.fileName !== descriptor.loader.name
  ) {
    throw new Error('Zvec qualification report target or loader name is inconsistent');
  }
  const runtime = catalogRuntime(bundle);
  const manifest = catalogManifest(bundle);
  const packedRuntime = artifactPath(bundle, runtime);
  const byteLength = fs.statSync(packedRuntime).size;
  const sha256 = sha256Of(packedRuntime);
  if (
    report.componentId !== runtime.component_id ||
    report.artifactId !== runtime.id ||
    report.loader.byteLength !== byteLength ||
    report.loader.sha256 !== sha256 ||
    catalogSha256(runtime.checksum) !== sha256 ||
    runtime.resources.download_bytes !== byteLength ||
    runtime.version !== ZVEC_VERSION ||
    runtime.compatibility.target.operating_system !== descriptor.operatingSystem ||
    runtime.compatibility.target.architecture !== descriptor.architecture
  ) {
    throw new Error('Zvec qualification report disagrees with the catalog or packed artifact');
  }
  const runtimeProvenance = manifest.provenance.find((record) => record.artifact_id === runtime.id);
  const worker = manifest.catalog.artifacts.find(
    (artifact) => artifact.component_id === 'procyon.semantic.worker',
  );
  const workerProvenance = manifest.provenance.find((record) => record.artifact_id === worker?.id);
  if (
    runtime.license.spdx !== 'Apache-2.0' ||
    runtimeProvenance?.source_revision !== ZVEC_RUST_REVISION ||
    !/^[a-f0-9]{40}$/u.test(report.build?.revision ?? '') ||
    !['clean', 'dirty'].includes(report.build?.workingTree) ||
    workerProvenance?.source_revision !== report.build.revision
  ) {
    throw new Error('Zvec qualification source, build, or license provenance is inconsistent');
  }
  const dependencyEvidence = verifyRuntimeBinary(packedRuntime, descriptor);
  if (!sameJson(report.dynamicDependencies, dependencyEvidence)) {
    throw new Error('Zvec qualification dependency evidence does not match the packed artifact');
  }
  const trustStatus = `${report.signing?.status}/${report.notarization?.status}`;
  const allowedTrustStatus =
    descriptor.platform === 'darwin'
      ? [
          'unsigned-local-build/not-requested',
          'developer-id-verified/pending-apple-notary-service',
          'developer-id-verified/apple-notary-service-accepted',
        ]
      : descriptor.platform === 'win32'
        ? ['unsigned/not-applicable']
        : ['not-applicable/not-applicable'];
  if (!allowedTrustStatus.includes(trustStatus)) {
    throw new Error('Zvec qualification signing or notarization status is inconsistent');
  }
  if (descriptor.platform === 'darwin' && report.signing.status === 'developer-id-verified') {
    for (const artifact of notarizedArtifactEvidence(bundle, manifest)) {
      verifyMacDeveloperIdSignature(path.join(bundle, 'artifacts', artifact.artifactId));
    }
  }
  if (
    report.notarization.status === 'apple-notary-service-accepted' &&
    (!/^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/iu.test(
      report.notarization.submissionId ?? '',
    ) ||
      !Number.isSafeInteger(report.notarization.submittedArchive?.byteLength) ||
      report.notarization.submittedArchive.byteLength <= 0 ||
      !/^[a-f0-9]{64}$/u.test(report.notarization.submittedArchive?.sha256 ?? '') ||
      !sameJson(report.notarization.artifacts, notarizedArtifactEvidence(bundle, manifest)))
  ) {
    throw new Error('Apple notarization evidence is not bound to the packaged artifacts');
  }
  if (
    requireProductionTrust &&
    descriptor.platform === 'darwin' &&
    trustStatus !== 'developer-id-verified/apple-notary-service-accepted'
  ) {
    throw new Error('macOS Zvec runtime lacks verified Developer ID and notarization evidence');
  }
  return { descriptor, report, runtime, packedRuntime };
}

export function recordAcceptedMacNotarization(bundle, resultFile, submittedArchive) {
  const result = JSON.parse(fs.readFileSync(resultFile, 'utf8'));
  if (
    result.status !== 'Accepted' ||
    !/^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/iu.test(result.id ?? '')
  ) {
    throw new Error('Apple notarization result is not an accepted submission with an identifier');
  }
  const reportPath = path.join(bundle, 'zvec-runtime-qualification.json');
  const report = JSON.parse(fs.readFileSync(reportPath, 'utf8'));
  if (
    report.target?.operatingSystem !== 'macos' ||
    report.signing?.status !== 'developer-id-verified' ||
    report.notarization?.status !== 'pending-apple-notary-service'
  ) {
    throw new Error('only a Developer ID verified macOS runtime can record notarization');
  }
  verifyZvecRuntimeQualification(bundle);
  const manifest = catalogManifest(bundle);
  const artifacts = verifySubmittedNotarizationArchive(bundle, manifest, submittedArchive);
  report.notarization = {
    status: 'apple-notary-service-accepted',
    submissionId: result.id,
    submittedArchive: {
      byteLength: fs.statSync(submittedArchive).size,
      sha256: sha256Of(submittedArchive),
    },
    artifacts,
  };
  fs.writeFileSync(reportPath, `${JSON.stringify(report, null, 2)}\n`);
}

async function main(args) {
  const [command, first, second, third] = args;
  if (command === 'prepare') {
    const descriptor = zvecRuntimeTarget();
    const cacheRoot = path.resolve(
      first ?? path.join(repositoryRoot, 'target', 'zvec-runtime-cache'),
    );
    const prepared = await preparePinnedZvecRuntime(descriptor, cacheRoot);
    console.log(prepared.library);
    return;
  }
  if (command === 'verify' && first) {
    verifyZvecRuntimeQualification(path.resolve(first), {
      requireProductionTrust: process.env.PROCYON_REQUIRE_ZVEC_PRODUCTION_TRUST === '1',
    });
    console.log(path.resolve(first));
    return;
  }
  if (command === 'record-notarization' && first && second && third) {
    recordAcceptedMacNotarization(path.resolve(first), path.resolve(second), path.resolve(third));
    console.log(path.resolve(first, 'zvec-runtime-qualification.json'));
    return;
  }
  throw new Error(
    'Usage: zvec-runtime-qualification.mjs prepare [cache] | verify <bundle> | ' +
      'record-notarization <bundle> <notary-result.json> <submitted.zip>',
  );
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  await main(process.argv.slice(2));
}
