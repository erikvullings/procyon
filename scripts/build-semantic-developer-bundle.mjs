import { execFileSync, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const repositoryRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

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

function findZvecNativeLibrary(targetDirectory, profile) {
  const names =
    process.platform === 'darwin'
      ? ['libzvec_c_api.dylib']
      : process.platform === 'win32'
        ? ['zvec_c_api.dll', 'libzvec_c_api.dll']
        : ['libzvec_c_api.so'];
  const buildDirectory = path.join(targetDirectory, profile, 'build');
  const candidates = fs
    .readdirSync(buildDirectory, { withFileTypes: true })
    .filter((entry) => entry.isDirectory() && entry.name.startsWith('zvec-rust-sys-'))
    .flatMap((entry) =>
      names.map((name) => path.join(buildDirectory, entry.name, 'out', 'zvec-prebuilt', name)),
    )
    .filter((candidate) => fs.existsSync(candidate))
    .sort((left, right) => fs.statSync(right).mtimeMs - fs.statSync(left).mtimeMs);
  if (candidates.length === 0) {
    throw new Error(
      `The Zvec native runtime was not emitted under ${buildDirectory}; rebuild with the developer-bundle feature and inspect the zvec-rust-sys build output.`,
    );
  }
  return candidates[0];
}

export function buildSemanticDeveloperBundle() {
  const supported =
    (process.platform === 'darwin' && process.arch === 'arm64') ||
    (process.platform === 'win32' && process.arch === 'x64') ||
    (process.platform === 'linux' && ['x64', 'arm64'].includes(process.arch));
  if (!supported) {
    throw new Error(
      `No official Zvec 0.7 native runtime is available for ${process.platform}-${process.arch}`,
    );
  }
  const metadata = JSON.parse(
    execFileSync('cargo', ['metadata', '--format-version=1', '--no-deps'], {
      cwd: repositoryRoot,
      env: process.env,
      encoding: 'utf8',
    }),
  );
  const profile = process.argv.includes('--release') ? 'release' : 'debug';
  const buildArgs = [
    'build',
    '-p',
    'fm-semantic-worker',
    '--features',
    'developer-bundle',
    '--bin',
    'fm-semantic-worker',
  ];
  if (profile === 'release') buildArgs.push('--release');
  run('cargo', buildArgs);

  const executable = path.join(
    metadata.target_directory,
    profile,
    process.platform === 'win32' ? 'fm-semantic-worker.exe' : 'fm-semantic-worker',
  );
  const output = path.join(
    metadata.target_directory,
    'semantic-developer-bundle',
    `${process.platform}-${process.arch}`,
  );
  const nativeRuntime = findZvecNativeLibrary(metadata.target_directory, profile);
  run('cargo', [
    'run',
    '-p',
    'fm-semantic-components',
    '--example',
    'build_semantic_developer_bundle',
    '--',
    executable,
    nativeRuntime,
    output,
  ]);
  return output;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const output = buildSemanticDeveloperBundle();
  console.log(`Semantic developer bundle: ${output}`);
}
