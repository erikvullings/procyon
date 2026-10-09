import { spawnSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { buildSemanticDeveloperBundle } from './build-semantic-developer-bundle.mjs';

const repositoryRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const enableGemma = process.argv.includes('--gemma');
const metalImages = process.argv.includes('--metal-images');
if (metalImages && !enableGemma) throw new Error('--metal-images requires --gemma');
const bundle = await buildSemanticDeveloperBundle();
const result = spawnSync(
  'pnpm',
  ['exec', 'tauri', 'dev', ...(enableGemma ? ['--features', 'semantic-gemma'] : [])],
  {
    cwd: path.join(repositoryRoot, 'apps/fm-desktop/src-tauri'),
    env: {
      ...process.env,
      PROCYON_SEMANTIC_COMPONENTS: '',
      PROCYON_SEMANTIC_DEVELOPER_BUNDLE: bundle,
      PROCYON_SEMANTIC_OCRMYPDF: '',
      PROCYON_SEMANTIC_GEMMA_METAL_IMAGES: metalImages ? '1' : '',
    },
    stdio: 'inherit',
  },
);
if (result.error) throw result.error;
process.exitCode = result.status ?? 1;
