import { spawnSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const repositoryRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const [command, ...args] = process.argv.slice(2);

if (!['sign', 'verify'].includes(command)) {
  throw new Error('Usage: semantic-production-catalog.mjs <sign|verify> <catalog arguments...>');
}

const result = spawnSync(
  'cargo',
  [
    'run',
    '--quiet',
    '-p',
    'fm-semantic-components',
    '--example',
    'semantic_production_catalog',
    '--',
    command,
    ...args,
  ],
  {
    cwd: repositoryRoot,
    env: process.env,
    stdio: 'inherit',
  },
);
if (result.error) throw result.error;
if (result.status !== 0) {
  throw new Error(`production catalog ${command} failed`);
}
