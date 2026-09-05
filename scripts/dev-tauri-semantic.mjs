import { spawnSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { buildSemanticDeveloperBundle } from './build-semantic-developer-bundle.mjs';

const repositoryRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const bundle = buildSemanticDeveloperBundle();
const result = spawnSync('pnpm', ['exec', 'tauri', 'dev'], {
  cwd: path.join(repositoryRoot, 'apps/fm-desktop/src-tauri'),
  env: {
    ...process.env,
    PROCYON_SEMANTIC_COMPONENTS: '',
    PROCYON_SEMANTIC_DEVELOPER_BUNDLE: bundle,
  },
  stdio: 'inherit',
});
if (result.error) throw result.error;
process.exitCode = result.status ?? 1;
