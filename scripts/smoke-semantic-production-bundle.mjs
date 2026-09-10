import { execFileSync, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';

import {
  ONNX_RUNTIME_COMPONENT_ID,
  onnxRuntimeTarget,
  preparePinnedOnnxRuntime,
  verifyOnnxRuntimeQualification,
} from './onnx-runtime-qualification.mjs';
import {
  preparePinnedZvecRuntime,
  verifyZvecRuntimeQualification,
} from './zvec-runtime-qualification.mjs';

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
const onnxRuntime = manifest.catalog.artifacts.find(
  (artifact) => artifact.component_id === ONNX_RUNTIME_COMPONENT_ID,
);
if (!model) throw new Error('production catalog has no multilingual model artifact');
if (!worker) throw new Error('production catalog has no worker artifact');
if (!runtime) throw new Error('production catalog has no Zvec runtime artifact');
const onnxDescriptor = onnxRuntimeTarget();
if (onnxDescriptor && !onnxRuntime) {
  throw new Error('Linux x86-64 production catalog has no ONNX Runtime artifact');
}
if (!onnxDescriptor && onnxRuntime) {
  throw new Error('production catalog has an unexpected ONNX Runtime artifact');
}
const modelPack = path.join(bundle, 'artifacts', model.id);
const workerExecutable = path.join(bundle, 'artifacts', worker.id);
const nativeRuntime = path.join(bundle, 'artifacts', runtime.id);
const qualification = verifyZvecRuntimeQualification(bundle, {
  requireProductionTrust: process.env.PROCYON_REQUIRE_ZVEC_PRODUCTION_TRUST === '1',
});
if (onnxDescriptor) verifyOnnxRuntimeQualification(bundle);
const cargoTarget = JSON.parse(
  execFileSync('cargo', ['metadata', '--format-version=1', '--no-deps'], {
    encoding: 'utf8',
  }),
).target_directory;
const preparedRuntime = await preparePinnedZvecRuntime(
  qualification.descriptor,
  path.join(cargoTarget, 'semantic-zvec-runtime-cache'),
);
const preparedOnnxRuntime = onnxDescriptor
  ? await preparePinnedOnnxRuntime(
      onnxDescriptor,
      path.join(cargoTarget, 'semantic-onnx-runtime-cache'),
    )
  : undefined;
const nativeRuntimeDirectories = [
  preparedRuntime.directory,
  ...(preparedOnnxRuntime ? [preparedOnnxRuntime.directory] : []),
];
const nativeRuntimePath = nativeRuntimeDirectories.join(path.delimiter);
const runtimeLoaderEnvironment =
  process.platform === 'darwin'
    ? { DYLD_LIBRARY_PATH: nativeRuntimePath }
    : process.platform === 'win32'
      ? { PATH: `${nativeRuntimePath}${path.delimiter}${process.env.PATH ?? ''}` }
      : { LD_LIBRARY_PATH: nativeRuntimePath };
const isolatedRuntimeDirectory = fs.mkdtempSync(
  path.join(tmpdir(), 'procyon-packaged-zvec-runtime-'),
);
const isolatedNativeRuntime = path.join(
  isolatedRuntimeDirectory,
  qualification.report.loader.fileName,
);
fs.copyFileSync(nativeRuntime, isolatedNativeRuntime);
if (onnxDescriptor) {
  fs.copyFileSync(
    path.join(bundle, 'artifacts', onnxRuntime.id),
    path.join(isolatedRuntimeDirectory, onnxDescriptor.loader.name),
  );
}

function run(args, environment = {}) {
  const result = spawnSync('cargo', args, {
    env: {
      ...process.env,
      ZVEC_AUTO_BUILD: '0',
      ZVEC_LIB_DIR: preparedRuntime.directory,
      ...(preparedOnnxRuntime
        ? {
            ORT_LIB_PATH: preparedOnnxRuntime.directory,
            ORT_PREFER_DYNAMIC_LINK: '1',
            ORT_SKIP_DOWNLOAD: '1',
          }
        : {}),
      ...runtimeLoaderEnvironment,
      ...environment,
    },
    stdio: 'inherit',
  });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    throw new Error(`cargo ${args.join(' ')} exited with status ${result.status ?? 'unknown'}`);
  }
}

try {
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
      PROCYON_SEMANTIC_PRODUCTION_NATIVE_RUNTIME: isolatedNativeRuntime,
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
} finally {
  fs.rmSync(isolatedRuntimeDirectory, { recursive: true, force: true });
}
