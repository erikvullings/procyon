#!/usr/bin/env node
// Pre-push fail-fast check: lint only (fmt + clippy + biome), so an obviously broken
// push is caught before it reaches CI. Deliberately does NOT run the full test suite -
// .github/workflows/ci.yml already runs it on every push, so running it here too just
// duplicates that cost and blocks local iteration for no extra safety. If you want to
// run tests before pushing, run `pnpm test` yourself.

import { spawnSync } from 'node:child_process';
import path from 'node:path';
import process from 'node:process';

const repositoryRoot = path.resolve(import.meta.dirname, '..');

function run(command, args) {
  console.log(`pre-push: running \`${command} ${args.join(' ')}\``);
  const result = spawnSync(command, args, {
    cwd: repositoryRoot,
    stdio: 'inherit',
    windowsHide: true,
    shell: process.platform === 'win32',
  });
  if (result.error !== undefined) {
    console.error(`pre-push: failed to run ${command}: ${result.error.message}`);
    process.exit(1);
  }
  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }
}

run('pnpm', ['run', 'lint']);
