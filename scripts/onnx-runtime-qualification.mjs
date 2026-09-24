import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { sha256Of, verifyPinnedFile } from './zvec-runtime-qualification.mjs';

const repositoryRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

export const ONNX_RUNTIME_VERSION = '1.28.0';
export const ONNX_RUNTIME_SOURCE_REVISION = 'da9b5e364c465de65c49d91e696cd6485270757f';
export const ONNX_RUNTIME_COMPONENT_ID = 'procyon.semantic.onnx-runtime';

const releaseBase = `https://github.com/microsoft/onnxruntime/releases/download/v${ONNX_RUNTIME_VERSION}`;
const packageRoot = `onnxruntime-linux-x64-${ONNX_RUNTIME_VERSION}`;

const linuxX64 = {
  platform: 'linux',
  processArchitecture: 'x64',
  operatingSystem: 'linux',
  architecture: 'x86_64',
  rustTarget: 'x86_64-unknown-linux-gnu',
  packageRoot,
  archive: {
    assetId: 489174677,
    name: `${packageRoot}.tgz`,
    bytes: 9_125_960,
    sha256: 'a3e1b79d7bb1bf09696ce675f49e4064e6c81f6202b8225624fff0e93f8d6407',
  },
  loader: {
    sourceName: `libonnxruntime.so.${ONNX_RUNTIME_VERSION}`,
    linkName: 'libonnxruntime.so',
    name: 'libonnxruntime.so.1',
    bytes: 24_268_848,
    sha256: '1461ef7cc3d9e49982591721683cc3e3a55580aeca9a5254e7aac47b75ee4bab',
    machine: 62,
  },
  providerShared: {
    name: 'libonnxruntime_providers_shared.so',
    bytes: 14_632,
    sha256: '086ec1d5388f64153d9c63470d126693db9a182c8ce236d3a1119068471b8a0d',
  },
  expectedDependencies: [
    'ld-linux-x86-64.so.2',
    'libc.so.6',
    'libdl.so.2',
    'libgcc_s.so.1',
    'libm.so.6',
    'libpthread.so.0',
    'librt.so.1',
    'libstdc++.so.6',
  ],
  abiCeilings: {
    glibc: '2.35',
    glibcxx: '3.4.30',
    cxxabi: '1.3.13',
  },
  sourceIdentity: {
    name: 'GIT_COMMIT_ID',
    bytes: 41,
    sha256: '40f2814e70e7d02460565104d089b42cf9e1df137495a4eae35654c4425840d7',
  },
  versionIdentity: {
    name: 'VERSION_NUMBER',
    bytes: 7,
    sha256: '229394bb3bac6e15d186324b7cbc90560803df8d7ee438a584118c4821b44e93',
  },
  license: {
    name: 'LICENSE',
    bytes: 1_073,
    sha256: '2f07c72751aed99790b8a4869cf2311df85a860b22ded05fa22803587a48922c',
  },
  thirdPartyNotices: {
    name: 'ThirdPartyNotices.txt',
    bytes: 325_054,
    sha256: '0e07b95f3a8d6230037707c5c4a2b554d12c4cb67369669ac255635528ffcee2',
  },
};

linuxX64.archive.url = `${releaseBase}/${linuxX64.archive.name}`;

export function onnxRuntimeTarget(platform = process.platform, processArchitecture = process.arch) {
  if (platform === linuxX64.platform && processArchitecture === linuxX64.processArchitecture) {
    return linuxX64;
  }
  return undefined;
}

function normalizedArchiveMember(member) {
  return member
    .replaceAll('\\', '/')
    .replace(/^\.\/+/u, '')
    .replace(/\/$/u, '');
}

export function verifyOnnxRuntimeArchiveMembers(members, descriptor) {
  const actual = new Set(members.map(normalizedArchiveMember).filter(Boolean));
  const prefix = `${descriptor.packageRoot}/`;
  const required = [
    descriptor.sourceIdentity.name,
    descriptor.versionIdentity.name,
    descriptor.license.name,
    descriptor.thirdPartyNotices.name,
    `lib/${descriptor.loader.linkName}`,
    `lib/${descriptor.loader.name}`,
    `lib/${descriptor.loader.sourceName}`,
    `lib/${descriptor.providerShared.name}`,
  ].map((member) => `${prefix}${member}`);
  const missing = required.filter((member) => !actual.has(member));
  if (missing.length > 0) {
    throw new Error(`ONNX Runtime native package is incomplete: missing ${missing.join(', ')}`);
  }
  const expectedNativeLibraries = new Set(required.filter((member) => member.includes('/lib/')));
  const unexpected = [...actual].filter(
    (member) =>
      member.startsWith(`${prefix}lib/libonnxruntime`) &&
      /\.so(?:\.|$)/u.test(member) &&
      !expectedNativeLibraries.has(member),
  );
  if (unexpected.length > 0) {
    throw new Error(
      `ONNX Runtime package has an unexpected native library: ${unexpected.join(', ')}`,
    );
  }
}

function compareVersions(left, right) {
  const leftParts = left.split('.').map(Number);
  const rightParts = right.split('.').map(Number);
  const length = Math.max(leftParts.length, rightParts.length);
  for (let index = 0; index < length; index += 1) {
    const difference = (leftParts[index] ?? 0) - (rightParts[index] ?? 0);
    if (difference !== 0) return Math.sign(difference);
  }
  return 0;
}

function maximumVersion(output, pattern) {
  const versions = [...output.matchAll(pattern)].map((match) => match[1]);
  versions.sort(compareVersions);
  return versions.at(-1);
}

export function verifyLinuxAbiCompatibility(
  versionInformation,
  dynamicSymbols,
  ceilings = linuxX64.abiCeilings,
) {
  const requirements = {
    glibc: maximumVersion(versionInformation, /\bGLIBC_([0-9]+(?:\.[0-9]+)+)\b/gu),
    glibcxx: maximumVersion(versionInformation, /\bGLIBCXX_([0-9]+(?:\.[0-9]+)+)\b/gu),
    cxxabi: maximumVersion(versionInformation, /\bCXXABI_([0-9]+(?:\.[0-9]+)+)\b/gu),
  };
  for (const [name, requirement] of Object.entries(requirements)) {
    if (requirement && compareVersions(requirement, ceilings[name]) > 0) {
      const label = name === 'glibc' ? 'GLIBC' : name === 'glibcxx' ? 'GLIBCXX' : 'CXXABI';
      throw new Error(
        `ONNX Runtime requires ${label}_${requirement}; Ubuntu 22.04 supports at most ` +
          `${label}_${ceilings[name]}`,
      );
    }
  }
  const forbidden = dynamicSymbols.match(
    /__isoc23_(?:strtol|strtoll|strtoull)|_M_replace_cold/gu,
  )?.[0];
  if (forbidden) {
    throw new Error(`ONNX Runtime uses forbidden Ubuntu 22.04 symbol ${forbidden}`);
  }
  return requirements;
}

function run(command, args) {
  const result = spawnSync(command, args, {
    cwd: repositoryRoot,
    env: process.env,
    encoding: 'utf8',
  });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    throw new Error(
      `${command} ${args.join(' ')} exited with status ${result.status ?? 'unknown'}\n` +
        `${result.stdout}${result.stderr}`,
    );
  }
  return result.stdout;
}

function parseElfDependencies(output) {
  return [...output.matchAll(/\(NEEDED\).*Shared library: \[([^\]]+)\]/gu)]
    .map((match) => match[1])
    .sort();
}

function verifyElfX64(file) {
  const bytes = fs.readFileSync(file);
  if (
    bytes.length < 20 ||
    bytes[0] !== 0x7f ||
    bytes.subarray(1, 4).toString('ascii') !== 'ELF' ||
    bytes[4] !== 2 ||
    bytes[5] !== 1 ||
    bytes.readUInt16LE(18) !== linuxX64.loader.machine
  ) {
    throw new Error('ONNX Runtime loader is not a little-endian x86-64 ELF binary');
  }
}

export function verifyOnnxRuntimeBinary(file, descriptor, { pinned = false } = {}) {
  if (pinned) verifyPinnedFile(file, descriptor.loader, 'pinned ONNX Runtime loader');
  verifyElfX64(file);
  const dynamicSection = run('readelf', ['-d', file]);
  const soname = dynamicSection.match(/\(SONAME\).*Library soname: \[([^\]]+)\]/u)?.[1];
  if (soname !== descriptor.loader.name) {
    throw new Error(
      `ONNX Runtime loader SONAME is ${soname ?? '(missing)'}; expected ${descriptor.loader.name}`,
    );
  }
  const dependencies = parseElfDependencies(dynamicSection);
  const expectedDependencies = [...descriptor.expectedDependencies].sort();
  if (JSON.stringify(dependencies) !== JSON.stringify(expectedDependencies)) {
    throw new Error(
      `ONNX Runtime dependencies are incomplete or unexpected: found ` +
        `${dependencies.join(', ') || '(none)'}; expected ${expectedDependencies.join(', ')}`,
    );
  }
  const versionInformation = run('readelf', ['--version-info', file]);
  const dynamicSymbols = run('readelf', ['--dyn-syms', '--wide', file]);
  const abi = verifyLinuxAbiCompatibility(
    versionInformation,
    dynamicSymbols,
    descriptor.abiCeilings,
  );
  return { tool: 'readelf', soname, dependencies, abi, ceilings: descriptor.abiCeilings };
}

function archiveMembers(archive) {
  return run('tar', ['-tzf', archive]).split(/\r?\n/u);
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

function verifySymlink(file, expectedTarget) {
  if (!fs.lstatSync(file).isSymbolicLink() || fs.readlinkSync(file) !== expectedTarget) {
    throw new Error(
      `ONNX Runtime loader alias ${path.basename(file)} does not target ${expectedTarget}`,
    );
  }
}

function verifyExtractedDirectory(root, descriptor) {
  const libraryDirectory = path.join(root, 'lib');
  const library = path.join(libraryDirectory, descriptor.loader.sourceName);
  verifyPinnedFile(library, descriptor.loader, 'pinned ONNX Runtime loader');
  verifyPinnedFile(
    path.join(libraryDirectory, descriptor.providerShared.name),
    descriptor.providerShared,
    'pinned ONNX Runtime provider support library',
  );
  for (const pinned of [
    descriptor.sourceIdentity,
    descriptor.versionIdentity,
    descriptor.license,
    descriptor.thirdPartyNotices,
  ]) {
    verifyPinnedFile(path.join(root, pinned.name), pinned, `pinned ONNX Runtime ${pinned.name}`);
  }
  if (
    fs.readFileSync(path.join(root, descriptor.sourceIdentity.name), 'utf8').trim() !==
    ONNX_RUNTIME_SOURCE_REVISION
  ) {
    throw new Error('ONNX Runtime package source revision is not the pinned release revision');
  }
  if (
    fs.readFileSync(path.join(root, descriptor.versionIdentity.name), 'utf8').trim() !==
    ONNX_RUNTIME_VERSION
  ) {
    throw new Error('ONNX Runtime package version does not match the pinned release');
  }
  verifySymlink(path.join(libraryDirectory, descriptor.loader.linkName), descriptor.loader.name);
  verifySymlink(path.join(libraryDirectory, descriptor.loader.name), descriptor.loader.sourceName);
  const dependencyEvidence = verifyOnnxRuntimeBinary(library, descriptor, { pinned: true });
  return {
    directory: libraryDirectory,
    library,
    linkLibrary: path.join(libraryDirectory, descriptor.loader.linkName),
    dependencyEvidence,
  };
}

export async function preparePinnedOnnxRuntime(descriptor, cacheRoot) {
  const root = path.resolve(cacheRoot);
  const archiveDirectory = path.join(root, 'archives');
  const extractedRoot = path.join(root, ONNX_RUNTIME_VERSION, descriptor.rustTarget);
  const packageDirectory = path.join(extractedRoot, descriptor.packageRoot);
  const archive = path.join(archiveDirectory, descriptor.archive.name);
  fs.mkdirSync(archiveDirectory, { recursive: true });

  if (fs.existsSync(archive)) {
    try {
      verifyPinnedFile(archive, descriptor.archive, 'cached ONNX Runtime release archive');
    } catch {
      fs.rmSync(archive, { force: true });
    }
  }
  if (!fs.existsSync(archive)) {
    await downloadPinnedFile(descriptor.archive.url, archive, descriptor.archive);
  }
  verifyOnnxRuntimeArchiveMembers(archiveMembers(archive), descriptor);

  if (fs.existsSync(packageDirectory)) {
    try {
      return verifyExtractedDirectory(packageDirectory, descriptor);
    } catch {
      fs.rmSync(extractedRoot, { recursive: true, force: true });
    }
  }

  const staging = `${extractedRoot}.extracting`;
  fs.rmSync(staging, { recursive: true, force: true });
  fs.mkdirSync(staging, { recursive: true });
  try {
    run('tar', ['-xzf', archive, '-C', staging]);
    const verified = verifyExtractedDirectory(
      path.join(staging, descriptor.packageRoot),
      descriptor,
    );
    fs.mkdirSync(path.dirname(extractedRoot), { recursive: true });
    fs.renameSync(staging, extractedRoot);
    return {
      ...verified,
      directory: path.join(packageDirectory, 'lib'),
      library: path.join(packageDirectory, 'lib', descriptor.loader.sourceName),
      linkLibrary: path.join(packageDirectory, 'lib', descriptor.loader.linkName),
    };
  } catch (error) {
    fs.rmSync(staging, { recursive: true, force: true });
    throw error;
  }
}

function catalogManifest(bundle) {
  return JSON.parse(fs.readFileSync(path.join(bundle, 'catalog-input.json'), 'utf8'));
}

function catalogSha256(value) {
  if (!Array.isArray(value) || value.length !== 32) {
    throw new Error('catalog SHA-256 does not contain 32 bytes');
  }
  return Buffer.from(value).toString('hex');
}

function expectedUpstreamMetadata(descriptor) {
  return {
    repository: 'https://github.com/microsoft/onnxruntime',
    revision: ONNX_RUNTIME_SOURCE_REVISION,
    tag: `v${ONNX_RUNTIME_VERSION}`,
    releaseAsset: {
      assetId: descriptor.archive.assetId,
      name: descriptor.archive.name,
      url: descriptor.archive.url,
      byteLength: descriptor.archive.bytes,
      sha256: descriptor.archive.sha256,
      loaderSourceName: descriptor.loader.sourceName,
      loaderByteLength: descriptor.loader.bytes,
      loaderSha256: descriptor.loader.sha256,
    },
    redistribution: {
      spdx: 'MIT',
      upstreamSourceModified: false,
      artifactTransformation: 'content-addressed rename plus SONAME loader alias at activation',
      license: descriptor.license,
      thirdPartyNotices: descriptor.thirdPartyNotices,
    },
  };
}

function catalogRuntime(bundle) {
  const manifest = catalogManifest(bundle);
  const artifact = manifest.catalog.artifacts.find(
    (candidate) => candidate.component_id === ONNX_RUNTIME_COMPONENT_ID,
  );
  if (!artifact) throw new Error('production catalog has no ONNX Runtime artifact');
  return { manifest, artifact };
}

export function writeOnnxRuntimeQualification({
  bundle,
  descriptor,
  procyonRevision,
  workingTreeStatus,
  dependencyEvidence,
}) {
  const { artifact } = catalogRuntime(bundle);
  const packedRuntime = path.join(bundle, 'artifacts', artifact.id);
  const byteLength = fs.statSync(packedRuntime).size;
  const sha256 = sha256Of(packedRuntime);
  if (
    byteLength !== descriptor.loader.bytes ||
    sha256 !== descriptor.loader.sha256 ||
    artifact.resources.download_bytes !== byteLength ||
    catalogSha256(artifact.checksum) !== sha256
  ) {
    throw new Error('packed ONNX Runtime bytes disagree with pinned catalog metadata');
  }
  const report = {
    formatVersion: 1,
    componentId: ONNX_RUNTIME_COMPONENT_ID,
    artifactId: artifact.id,
    onnxRuntimeVersion: ONNX_RUNTIME_VERSION,
    linkage: 'dynamic-packaged-runtime',
    target: {
      operatingSystem: descriptor.operatingSystem,
      architecture: descriptor.architecture,
      rustTarget: descriptor.rustTarget,
    },
    loader: {
      fileName: descriptor.loader.name,
      sourceFileName: descriptor.loader.sourceName,
      byteLength,
      sha256,
    },
    upstream: expectedUpstreamMetadata(descriptor),
    build: {
      source: 'https://github.com/erikvullings/procyon',
      revision: procyonRevision,
      workingTree: workingTreeStatus,
    },
    dynamicDependencies: dependencyEvidence,
    signing: { status: 'not-applicable' },
    notarization: { status: 'not-applicable' },
  };
  fs.writeFileSync(
    path.join(bundle, 'onnx-runtime-qualification.json'),
    `${JSON.stringify(report, null, 2)}\n`,
  );
  return report;
}

export function verifyOnnxRuntimeQualification(bundle) {
  const report = JSON.parse(
    fs.readFileSync(path.join(bundle, 'onnx-runtime-qualification.json'), 'utf8'),
  );
  const descriptor = onnxRuntimeTarget('linux', 'x64');
  const { manifest, artifact } = catalogRuntime(bundle);
  const packedRuntime = path.join(bundle, 'artifacts', artifact.id);
  const byteLength = fs.statSync(packedRuntime).size;
  const sha256 = sha256Of(packedRuntime);
  const provenance = manifest.provenance.find((record) => record.artifact_id === artifact.id);
  if (
    report.formatVersion !== 1 ||
    report.componentId !== ONNX_RUNTIME_COMPONENT_ID ||
    report.artifactId !== artifact.id ||
    report.onnxRuntimeVersion !== ONNX_RUNTIME_VERSION ||
    report.linkage !== 'dynamic-packaged-runtime' ||
    report.target?.rustTarget !== descriptor.rustTarget ||
    report.loader?.fileName !== descriptor.loader.name ||
    report.loader?.sourceFileName !== descriptor.loader.sourceName ||
    report.loader?.byteLength !== byteLength ||
    report.loader?.sha256 !== sha256 ||
    byteLength !== descriptor.loader.bytes ||
    sha256 !== descriptor.loader.sha256 ||
    artifact.version !== ONNX_RUNTIME_VERSION ||
    artifact.license.spdx !== 'MIT' ||
    artifact.resources.download_bytes !== byteLength ||
    catalogSha256(artifact.checksum) !== sha256 ||
    provenance?.source_revision !== ONNX_RUNTIME_SOURCE_REVISION ||
    JSON.stringify(report.upstream) !== JSON.stringify(expectedUpstreamMetadata(descriptor)) ||
    !/^[a-f0-9]{40}$/u.test(report.build?.revision ?? '') ||
    !['clean', 'dirty'].includes(report.build?.workingTree) ||
    report.signing?.status !== 'not-applicable' ||
    report.notarization?.status !== 'not-applicable'
  ) {
    throw new Error('ONNX Runtime qualification report disagrees with pinned packaged evidence');
  }
  const dependencyEvidence = verifyOnnxRuntimeBinary(packedRuntime, descriptor, { pinned: true });
  if (JSON.stringify(report.dynamicDependencies) !== JSON.stringify(dependencyEvidence)) {
    throw new Error('ONNX Runtime qualification ABI evidence does not match the packed artifact');
  }
  return { descriptor, report, artifact, packedRuntime };
}

async function main(args) {
  const [command, first] = args;
  if (command === 'prepare') {
    const descriptor = onnxRuntimeTarget();
    if (!descriptor) {
      throw new Error('the packaged ONNX Runtime input is required only on Linux x86-64');
    }
    const cacheRoot = path.resolve(
      first ?? path.join(repositoryRoot, 'target', 'onnx-runtime-cache'),
    );
    const prepared = await preparePinnedOnnxRuntime(descriptor, cacheRoot);
    console.log(prepared.library);
    return;
  }
  if (command === 'verify' && first) {
    verifyOnnxRuntimeQualification(path.resolve(first));
    console.log(path.resolve(first));
    return;
  }
  throw new Error('Usage: onnx-runtime-qualification.mjs prepare [cache] | verify <bundle>');
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  await main(process.argv.slice(2));
}
