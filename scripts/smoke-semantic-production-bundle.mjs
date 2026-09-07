import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';

const bundle = path.resolve(process.argv[2] ?? '');
if (!process.argv[2]) {
  throw new Error('Usage: smoke-semantic-production-bundle.mjs <bundle-directory>');
}
const manifest = JSON.parse(fs.readFileSync(path.join(bundle, 'catalog-input.json'), 'utf8'));
const model = manifest.catalog.artifacts.find(
  (artifact) => artifact.component_id === 'procyon.semantic.model.multilingual-e5-small',
);
const worker = manifest.catalog.artifacts.find(
  (artifact) => artifact.component_id === 'procyon.semantic.worker',
);
const runtime = manifest.catalog.artifacts.find(
  (artifact) => artifact.component_id === 'procyon.semantic.zvec-runtime',
);
if (!model) throw new Error('production catalog has no multilingual model artifact');
if (!worker) throw new Error('production catalog has no worker artifact');
if (!runtime) throw new Error('production catalog has no Zvec runtime artifact');
const modelPack = path.join(bundle, 'artifacts', model.id);
const workerExecutable = path.join(bundle, 'artifacts', worker.id);
const nativeRuntime = path.join(bundle, 'artifacts', runtime.id);

function run(args, environment = {}) {
  const result = spawnSync('cargo', args, {
    env: { ...process.env, ...environment },
    stdio: 'inherit',
  });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    throw new Error(`cargo ${args.join(' ')} exited with status ${result.status ?? 'unknown'}`);
  }
}

run(
  [
    'test',
    '--locked',
    '-p',
    'fm-semantic-worker',
    '--features',
    'semantic-runtime',
    '--test',
    'ipc',
    'packaged_production_worker_starts_negotiates_and_shuts_down',
    '--',
    '--ignored',
  ],
  {
    PROCYON_SEMANTIC_PRODUCTION_WORKER: workerExecutable,
    PROCYON_SEMANTIC_PRODUCTION_NATIVE_RUNTIME: nativeRuntime,
    PROCYON_SEMANTIC_PRODUCTION_MODEL_PACK: modelPack,
  },
);
run(
  [
    'test',
    '--locked',
    '-p',
    'fm-semantic-worker',
    '--features',
    'semantic-runtime',
    '--lib',
    'production_model_pack_activates_offline',
    '--',
    '--ignored',
  ],
  { PROCYON_SEMANTIC_PRODUCTION_MODEL_PACK: modelPack },
);
run(['test', '--locked', '-p', 'fm-semantic-components']);
