#!/usr/bin/env node
// Pre-commit checks, in Node rather than POSIX shell so macOS, Linux and
// Windows run the same code path. The previous shell hook relied on `xargs`,
// `dirname`, `grep` and nested subshells, which Git Bash's fork emulation
// fails to spawn reliably on Windows.

import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import path from 'node:path';
import process from 'node:process';

const repositoryRoot = path.resolve(import.meta.dirname, '..');

/** Biome's own JS entry point. Spawning the `.cmd` shim `pnpm exec` would use is
 * rejected by Node on Windows unless a shell is involved. */
const biomeEntry = 'node_modules/@biomejs/biome/bin/biome';

function run(command, args) {
  const result = spawnSync(command, args, {
    cwd: repositoryRoot,
    stdio: 'inherit',
    windowsHide: true,
  });
  if (result.error !== undefined) {
    console.error(`pre-commit: failed to run ${command}: ${result.error.message}`);
    process.exit(1);
  }
  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }
}

function stagedFiles(patterns) {
  const result = spawnSync(
    'git',
    ['diff', '--cached', '--name-only', '--diff-filter=ACM', '--', ...patterns],
    { cwd: repositoryRoot, encoding: 'utf8', windowsHide: true },
  );
  if (result.status !== 0) {
    console.error(`pre-commit: git diff failed: ${result.stderr ?? ''}`);
    process.exit(result.status ?? 1);
  }
  return result.stdout.split('\n').filter((line) => line.length > 0);
}

/** The nearest ancestor `Cargo.toml`, as a repository-relative path. */
function owningManifest(file) {
  let directory = path.posix.dirname(file);
  while (directory !== '.' && !existsSync(path.join(repositoryRoot, directory, 'Cargo.toml'))) {
    directory = path.posix.dirname(directory);
  }
  return directory === '.' ? 'Cargo.toml' : `${directory}/Cargo.toml`;
}

const stagedRustSources = stagedFiles(['*.rs']);
if (stagedRustSources.length > 0) {
  run('rustfmt', ['--edition', '2024', ...stagedRustSources]);
  run('git', ['add', ...stagedRustSources]);
}

// Clippy only, not `cargo test`: clippy type-checks and lints without executing
// anything, so it stays fast on an incremental build. The full test suite
// (including slow integration tests - large-tree copies, real SFTP sessions,
// etc.) only runs in CI now (see scripts/pre-push.mjs), not in a local hook -
// run `pnpm test` yourself before pushing if you want it to run locally too.
const stagedRust = stagedFiles(['*.rs', '**/Cargo.toml', 'Cargo.toml', 'Cargo.lock']);
if (stagedRust.length > 0) {
  const manifests = [...new Set(stagedRust.map(owningManifest))];
  for (const manifest of manifests) {
    if (manifest === 'Cargo.toml') {
      run('cargo', ['clippy', '--workspace', '--all-targets', '--', '-D', 'warnings']);
    } else {
      run('cargo', [
        'clippy',
        '--manifest-path',
        manifest,
        '--all-targets',
        '--',
        '-D',
        'warnings',
      ]);
    }
  }
}

const stagedBiome = stagedFiles([
  'frontend/**/*.ts',
  'frontend/**/*.tsx',
  'frontend/**/*.css',
  'frontend/**/*.json',
  'scripts/**/*.mjs',
  '*.json',
]).filter((file) => !file.startsWith('frontend/src/api/generated/'));
if (stagedBiome.length > 0) {
  run(process.execPath, [
    biomeEntry,
    'check',
    '--write',
    '--no-errors-on-unmatched',
    ...stagedBiome,
  ]);
  run('git', ['add', ...stagedBiome]);
}
