import { execFileSync, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { fetchMultilingualModel } from './fetch-semantic-model.mjs';

const repositoryRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

export const PRODUCTION_CHUNKER_IDENTITY = 'structural/3';
export const PRODUCTION_CONVERTER_IDENTITY = 'docling-pdf/1036000+baseline/2';

const targets = new Map([
  ['darwin-arm64', { os: 'macos', arch: 'aarch64', rust: 'aarch64-apple-darwin' }],
  ['win32-x64', { os: 'windows', arch: 'x86_64', rust: 'x86_64-pc-windows-msvc' }],
  ['linux-x64', { os: 'linux', arch: 'x86_64', rust: 'x86_64-unknown-linux-gnu' }],
  ['linux-arm64', { os: 'linux', arch: 'aarch64', rust: 'aarch64-unknown-linux-gnu' }],
]);

export function supportedSemanticTarget(platform = process.platform, architecture = process.arch) {
  const target = targets.get(`${platform}-${architecture}`);
  if (!target) {
    throw new Error(
      `No production semantic payload is supported for ${platform}-${architecture}; ` +
        'supported targets are macOS arm64, Windows x64, Linux x64, and Linux arm64.',
    );
  }
  return target;
}

export function nativeLibraryNames(platform = process.platform) {
  if (platform === 'darwin') return ['libzvec_c_api.dylib'];
  if (platform === 'win32') return ['zvec_c_api.dll', 'libzvec_c_api.dll'];
  return ['libzvec_c_api.so'];
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
  if (target.os !== 'macos') return;

  const identity = process.env.PROCYON_APPLE_SIGNING_IDENTITY;
  if (!identity) {
    if (process.env.PROCYON_REQUIRE_PLATFORM_SIGNING === '1') {
      throw new Error(
        'PROCYON_APPLE_SIGNING_IDENTITY is required for a production macOS semantic payload',
      );
    }
    return;
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
  }
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

function findZvecNativeLibrary(buildRoot, platform = process.platform) {
  const buildDirectory = path.join(buildRoot, 'release', 'build');
  const candidates = fs
    .readdirSync(buildDirectory, { withFileTypes: true })
    .filter((entry) => entry.isDirectory() && entry.name.startsWith('zvec-rust-sys-'))
    .flatMap((entry) =>
      nativeLibraryNames(platform).map((name) =>
        path.join(buildDirectory, entry.name, 'out', 'zvec-prebuilt', name),
      ),
    )
    .filter((candidate) => fs.existsSync(candidate))
    .sort((left, right) => fs.statSync(right).mtimeMs - fs.statSync(left).mtimeMs);
  if (candidates.length === 0) {
    throw new Error(`The pinned Zvec native runtime was not emitted under ${buildDirectory}`);
  }
  return candidates[0];
}

function parseArguments(args) {
  const values = new Map();
  for (let index = 0; index < args.length; index += 2) {
    const name = args[index];
    const value = args[index + 1];
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

export async function buildSemanticProductionBundle(args = process.argv.slice(2)) {
  const values = parseArguments(args);
  const target = supportedSemanticTarget();
  const cargoTarget = targetDirectory();
  const buildRoot = path.join(cargoTarget, target.rust);
  const modelCache = await fetchMultilingualModel(path.join(cargoTarget, 'semantic-model-cache'));
  run('rustup', ['target', 'add', target.rust]);
  run('cargo', [
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
  ]);
  const executable = path.join(
    buildRoot,
    'release',
    process.platform === 'win32' ? 'fm-semantic-worker.exe' : 'fm-semantic-worker',
  );
  const runtime = findZvecNativeLibrary(buildRoot);
  applyPlatformSigning(target, executable, runtime);
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
  return path.resolve(values.get('--output'));
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const output = await buildSemanticProductionBundle();
  console.log(output);
}
