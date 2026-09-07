// Runs the developer bundle's real-model tests, which are `#[ignore]` by
// default because they need the pinned multi-hundred-megabyte model pack.
//
// The pack path is resolved from the built bundle rather than guessed, so the
// tests fail loudly on a missing bundle instead of passing vacuously.

import { execFileSync, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const repositoryRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const MODEL_ARTIFACT = 'procyon.dev.model.multilingual-e5-small.v1';

const metadata = JSON.parse(
  execFileSync('cargo', ['metadata', '--format-version=1', '--no-deps'], {
    cwd: repositoryRoot,
    env: process.env,
    encoding: 'utf8',
  }),
);
const pack = path.join(
  metadata.target_directory,
  'semantic-developer-bundle',
  `${process.platform}-${process.arch}`,
  'artifacts',
  MODEL_ARTIFACT,
);
if (!fs.existsSync(pack)) {
  throw new Error(`Run \`pnpm semantic:bundle:dev\` first; ${pack} does not exist.`);
}

const result = spawnSync(
  'cargo',
  [
    'test',
    '-p',
    'fm-semantic-worker',
    '--features',
    'developer-bundle',
    '--lib',
    'developer_bundle::tests::real_multilingual',
    '--',
    '--ignored',
    '--nocapture',
  ],
  {
    cwd: repositoryRoot,
    env: { ...process.env, PROCYON_SEMANTIC_MODEL_PACK: pack },
    stdio: 'inherit',
  },
);
if (result.error) throw result.error;
process.exitCode = result.status ?? 1;
