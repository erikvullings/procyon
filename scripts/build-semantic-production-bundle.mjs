import { execFileSync, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { fetchMultilingualModel } from './fetch-semantic-model.mjs';
import {
  nativeLibraryNames,
  preparePinnedZvecRuntime,
  verifyMacDeveloperIdSignature,
  verifyRuntimeBinary,
  writeZvecRuntimeQualification,
  zvecRuntimeTarget,
} from './zvec-runtime-qualification.mjs';

export { nativeLibraryNames };

const repositoryRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

export const PRODUCTION_CHUNKER_IDENTITY = 'structural/3';
export const PRODUCTION_CONVERTER_IDENTITY = 'docling-pdf/1036000+baseline/2';

export function supportedSemanticTarget(platform = process.platform, architecture = process.arch) {
  const target = zvecRuntimeTarget(platform, architecture);
  return { os: target.operatingSystem, arch: target.architecture, rust: target.rustTarget };
}

function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    cwd: repositoryRoot,
    env: process.env,
    stdio: 'inherit',
    ...options,
  });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    throw new Error(`${command} exited with status ${result.status ?? 'unknown'}`);
  }
}

function applyPlatformSigning(target, executable, runtime) {
  if (target.os === 'windows') {
    return { signingStatus: 'unsigned', notarizationStatus: 'not-applicable' };
  }
  if (target.os !== 'macos') {
    return { signingStatus: 'not-applicable', notarizationStatus: 'not-applicable' };
  }

  const identity = process.env.PROCYON_APPLE_SIGNING_IDENTITY;
  if (!identity) {
    if (process.env.PROCYON_REQUIRE_PLATFORM_SIGNING === '1') {
      throw new Error(
        'PROCYON_APPLE_SIGNING_IDENTITY is required for a production macOS semantic payload',
      );
    }
    return { signingStatus: 'unsigned-local-build', notarizationStatus: 'not-requested' };
  }

  for (const payload of [runtime, executable]) {
    run('codesign', [
      '--force',
      '--options',
      'runtime',
      '--timestamp',
      '--sign',
      identity,
      payload,
    ]);
    verifyMacDeveloperIdSignature(payload);
  }
  return {
    signingStatus: 'developer-id-verified',
    notarizationStatus: 'pending-apple-notary-service',
  };
}

function targetDirectory() {
  return JSON.parse(
    execFileSync('cargo', ['metadata', '--format-version=1', '--no-deps'], {
      cwd: repositoryRoot,
      env: process.env,
      encoding: 'utf8',
    }),
  ).target_directory;
}

export function sourceBuildIdentity(
  revision,
  { requireClean = process.env.PROCYON_REQUIRE_CLEAN_SOURCE === '1' } = {},
) {
  if (!/^[a-f0-9]{40}$/u.test(revision)) {
    throw new Error('--source-revision must be a complete lowercase git commit SHA');
  }
  const head = execFileSync('git', ['rev-parse', 'HEAD'], {
    cwd: repositoryRoot,
    encoding: 'utf8',
  }).trim();
  if (head !== revision) {
    throw new Error(`--source-revision ${revision} does not match checked-out HEAD ${head}`);
  }
  const dirty =
    execFileSync('git', ['status', '--porcelain', '--untracked-files=all'], {
      cwd: repositoryRoot,
      encoding: 'utf8',
    }).trim().length > 0;
  if (requireClean && dirty) {
    throw new Error('production semantic payloads require a clean source checkout');
  }
  return { revision, workingTree: dirty ? 'dirty' : 'clean' };
}

export function parseProductionBundleArguments(args) {
  const normalizedArgs = args[0] === '--' ? args.slice(1) : args;
  const values = new Map();
  for (let index = 0; index < normalizedArgs.length; index += 2) {
    const name = normalizedArgs[index];
    const value = normalizedArgs[index + 1];
    if (!name?.startsWith('--') || !value) {
      throw new Error(
        'Usage: build-semantic-production-bundle.mjs --output <dir> ' +
          '--release-base-url <https-url> --source-revision <git-sha>',
      );
    }
    values.set(name, value);
  }
  for (const required of ['--output', '--release-base-url', '--source-revision']) {
    if (!values.has(required)) throw new Error(`${required} is required`);
  }
  return values;
}

function smokeExecutable(executable, runtime) {
  const environment = { ...process.env };
  const runtimeDirectory = path.dirname(runtime);
  if (process.platform === 'darwin') {
    environment.DYLD_LIBRARY_PATH = runtimeDirectory;
  } else if (process.platform === 'win32') {
    environment.PATH = `${runtimeDirectory}${path.delimiter}${environment.PATH ?? ''}`;
  } else {
    environment.LD_LIBRARY_PATH = runtimeDirectory;
  }
  const result = spawnSync(executable, ['--release-smoke-check'], {
    cwd: repositoryRoot,
    env: environment,
    encoding: 'utf8',
  });
  if (result.error) throw result.error;
  if (result.status === 0 || !result.stderr.includes('unknown semantic worker argument')) {
    throw new Error('the packaged worker did not reach its argument parser with the Zvec runtime');
  }
}

function artifactByComponent(bundle, componentId) {
  const manifest = JSON.parse(fs.readFileSync(path.join(bundle, 'catalog-input.json'), 'utf8'));
  const artifact = manifest.catalog.artifacts.find(
    (candidate) => candidate.component_id === componentId,
  );
  if (!artifact) throw new Error(`production catalog has no ${componentId} artifact`);
  return path.join(bundle, 'artifacts', artifact.id);
}

function smokePackagedExecutableOffline(bundle, target, sourceRuntimeDirectory) {
  const worker = artifactByComponent(bundle, 'procyon.semantic.worker');
  const runtime = artifactByComponent(bundle, 'procyon.semantic.zvec-runtime');
  const isolated = fs.mkdtempSync(path.join(os.tmpdir(), 'procyon-zvec-runtime-'));
  const isolatedRuntime = path.join(isolated, nativeLibraryNames(process.platform)[0]);
  const hiddenSource = `${sourceRuntimeDirectory}.offline-smoke-hidden`;
  fs.copyFileSync(runtime, isolatedRuntime);
  fs.rmSync(hiddenSource, { recursive: true, force: true });
  fs.renameSync(sourceRuntimeDirectory, hiddenSource);
  try {
    smokeExecutable(worker, isolatedRuntime);
  } finally {
    fs.renameSync(hiddenSource, sourceRuntimeDirectory);
    fs.rmSync(isolated, { recursive: true, force: true });
  }
  if (target.rustTarget !== zvecRuntimeTarget().rustTarget) {
    throw new Error('packaged runtime smoke target drifted from the current host');
  }
}

export async function buildSemanticProductionBundle(args = process.argv.slice(2)) {
  const values = parseProductionBundleArguments(args);
  const sourceIdentity = sourceBuildIdentity(values.get('--source-revision'));
  const target = supportedSemanticTarget();
  const runtimeTarget = zvecRuntimeTarget();
  const cargoTarget = targetDirectory();
  const buildRoot = path.join(cargoTarget, target.rust);
  const preparedRuntime = await preparePinnedZvecRuntime(
    runtimeTarget,
    path.join(cargoTarget, 'semantic-zvec-runtime-cache'),
  );
  const modelCache = await fetchMultilingualModel(path.join(cargoTarget, 'semantic-model-cache'));
  run('rustup', ['target', 'add', target.rust]);
  run(
    'cargo',
    [
      'build',
      '--locked',
      '--release',
      '--target',
      target.rust,
      '-p',
      'fm-semantic-worker',
      '--features',
      'semantic-runtime',
      '--bin',
      'fm-semantic-worker',
    ],
    {
      env: {
        ...process.env,
        ZVEC_AUTO_BUILD: '0',
        ZVEC_LIB_DIR: preparedRuntime.directory,
      },
    },
  );
  const executable = path.join(
    buildRoot,
    'release',
    process.platform === 'win32' ? 'fm-semantic-worker.exe' : 'fm-semantic-worker',
  );
  const runtimeDirectory = path.join(buildRoot, 'semantic-zvec-runtime');
  fs.rmSync(runtimeDirectory, { recursive: true, force: true });
  fs.mkdirSync(runtimeDirectory, { recursive: true });
  const runtime = path.join(runtimeDirectory, runtimeTarget.loader.name);
  fs.copyFileSync(preparedRuntime.library, runtime);
  const trust = applyPlatformSigning(target, executable, runtime);
  const dependencyEvidence = verifyRuntimeBinary(runtime, runtimeTarget);
  smokeExecutable(executable, runtime);

  run('cargo', [
    'run',
    '--quiet',
    '-p',
    'fm-semantic-components',
    '--example',
    'build_semantic_production_bundle',
    '--',
    executable,
    runtime,
    modelCache,
    path.resolve(values.get('--output')),
    target.os,
    target.arch,
    values.get('--release-base-url'),
    values.get('--source-revision'),
    PRODUCTION_CONVERTER_IDENTITY,
    PRODUCTION_CHUNKER_IDENTITY,
  ]);
  const output = path.resolve(values.get('--output'));
  smokePackagedExecutableOffline(output, runtimeTarget, preparedRuntime.directory);
  writeZvecRuntimeQualification({
    bundle: output,
    descriptor: runtimeTarget,
    procyonRevision: sourceIdentity.revision,
    workingTreeStatus: sourceIdentity.workingTree,
    dependencyEvidence,
    signingStatus: trust.signingStatus,
    notarizationStatus: trust.notarizationStatus,
  });
  return output;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const output = await buildSemanticProductionBundle();
  console.log(output);
}
