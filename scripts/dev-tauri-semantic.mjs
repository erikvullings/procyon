import { spawnSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { buildSemanticDeveloperBundle } from './build-semantic-developer-bundle.mjs';

const repositoryRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const enableGemma = process.argv.includes('--gemma');
const metalImages = process.argv.includes('--metal-images');
// A separate identity for the dev app, so it runs beside the installed release: its own single-instance
// lock and its own app data (settings, workspaces, semantic library), instead of handing off to or
// sharing them with the release.
const sideBySide = process.argv.includes('--side-by-side');
if (metalImages && !enableGemma) throw new Error('--metal-images requires --gemma');
const bundle = await buildSemanticDeveloperBundle();
const identity = sideBySide
  ? [
      '--config',
      JSON.stringify({ identifier: 'nl.erikvullings.procyon.dev', productName: 'Procyon Dev' }),
    ]
  : [];
const result = spawnSync(
  'pnpm',
  ['exec', 'tauri', 'dev', ...(enableGemma ? ['--features', 'semantic-gemma'] : []), ...identity],
  {
    cwd: path.join(repositoryRoot, 'apps/fm-desktop/src-tauri'),
    env: {
      ...process.env,
      PROCYON_SEMANTIC_COMPONENTS: '',
      PROCYON_SEMANTIC_DEVELOPER_BUNDLE: bundle,
      PROCYON_SEMANTIC_OCRMYPDF: '',
    },
    stdio: 'inherit',
  },
);
if (result.error) throw result.error;
process.exitCode = result.status ?? 1;
