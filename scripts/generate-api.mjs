#!/usr/bin/env node
// Generates the Fetch-based TypeScript client from frontend/openapi/openapi.json
// via Orval. Node rather than bash so macOS, Linux and Windows share one code
// path (on Windows `bash` on PATH is usually WSL).

import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import path from 'node:path';
import process from 'node:process';

const repositoryRoot = path.resolve(import.meta.dirname, '..');

if (!existsSync(path.join(repositoryRoot, 'frontend', 'orval.config.ts'))) {
  console.error(
    'error: orval client generation is not implemented until task 0010; see TASKS/0010-orval-client-generation.md',
  );
  process.exit(1);
}

// Orval's own JS entry point: spawning the `.cmd` shim `pnpm exec` would use is
// rejected by Node on Windows unless a shell is involved.
const result = spawnSync(
  process.execPath,
  [path.join('node_modules', 'orval', 'dist', 'bin', 'orval.mjs'), '--config', 'orval.config.ts'],
  { cwd: path.join(repositoryRoot, 'frontend'), stdio: 'inherit', windowsHide: true },
);

if (result.error !== undefined) {
  console.error(`error: failed to run orval: ${result.error.message}`);
  process.exit(1);
}
process.exit(result.status ?? 1);
