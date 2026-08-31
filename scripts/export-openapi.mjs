#!/usr/bin/env node
// Exports the backend OpenAPI document to frontend/openapi/openapi.json.
// Node rather than bash so macOS, Linux and Windows share one code path: on
// Windows `bash` on PATH is usually WSL, a different machine without this
// toolchain installed.

import { spawnSync } from 'node:child_process';
import path from 'node:path';
import process from 'node:process';

const repositoryRoot = path.resolve(import.meta.dirname, '..');
const outputPath = process.argv[2] ?? 'frontend/openapi/openapi.json';

const result = spawnSync(
  'cargo',
  ['run', '--quiet', '-p', 'fm-server', '--', 'export-openapi', outputPath],
  { cwd: repositoryRoot, encoding: 'utf8', windowsHide: true },
);

if (result.error !== undefined) {
  console.error(`error: failed to run cargo: ${result.error.message}`);
  process.exit(1);
}

const output = `${result.stdout ?? ''}${result.stderr ?? ''}`;
if (result.status !== 0) {
  process.stderr.write(output);
  process.exit(result.status ?? 1);
}
// Guards against the export subcommand silently no-op'ing.
if (output.includes('not implemented yet')) {
  console.error(
    'error: fm-server export-openapi is not implemented until task 0009; see TASKS/0009-openapi-export-command.md',
  );
  process.exit(1);
}

process.stdout.write(output);
