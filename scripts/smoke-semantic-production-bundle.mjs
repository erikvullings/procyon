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
const generatedCanaryFile = process.env.PROCYON_SEMANTIC_PRIVACY_CANARIES_FILE
  ? undefined
  : path.join(isolatedRuntimeDirectory, 'privacy-canaries.json');
if (generatedCanaryFile) {
  fs.writeFileSync(
    generatedCanaryFile,
    JSON.stringify({
      query: 'qualification-query-canary',
      excerpt: 'qualification-excerpt-canary',
      'filename-path': 'qualification/filename-path-canary.txt',
      prompt: 'qualification-prompt-canary',
      response: 'qualification-response-canary',
      credential: 'qualification-credential-canary',
      token: 'qualification-token-canary',
      'authorization-header': 'qualification-authorization-header-canary',
      'model-payload': 'qualification-model-payload-canary',
    }),
    { mode: 0o600 },
  );
}
const evidenceRoot = process.env.PROCYON_QUALIFICATION_EVIDENCE_ROOT
  ? path.resolve(process.env.PROCYON_QUALIFICATION_EVIDENCE_ROOT)
  : undefined;
if (evidenceRoot) fs.mkdirSync(evidenceRoot, { recursive: true });
let commandIndex = 0;
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
  commandIndex += 1;
  const output = evidenceRoot
    ? fs.openSync(path.join(evidenceRoot, `worker-command-${commandIndex}.log`), 'w')
    : undefined;
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
    stdio: output === undefined ? 'inherit' : ['ignore', output, output],
  });
  if (output !== undefined) fs.closeSync(output);
  if (result.error) throw result.error;
  if (result.status !== 0) {
    throw new Error(
      `cargo ${args.join(' ')} exited with status ${result.status ?? 'unknown'}; output retained in qualification evidence`,
    );
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
      'packaged_production',
      'packaged_worker_ingests_recovers_after_crash_and_reopens_offline',
      '--',
      '--ignored',
    ],
    {
      PROCYON_SEMANTIC_PRODUCTION_WORKER: workerExecutable,
      PROCYON_SEMANTIC_PRODUCTION_NATIVE_DIRECTORY: isolatedRuntimeDirectory,
      PROCYON_SEMANTIC_PRODUCTION_MODEL_PACK: modelPack,
      PROCYON_SEMANTIC_PRIVACY_CANARIES_FILE:
        process.env.PROCYON_SEMANTIC_PRIVACY_CANARIES_FILE ?? generatedCanaryFile,
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
  for (const testName of [
    'zvec_storage::tests::rebuilds_a_missing_derived_collection_from_authoritative_records',
    'zvec_storage::tests::rejects_a_content_field_with_the_wrong_index_type',
    'zvec_storage::tests::migration_rolls_back_an_unpublished_staging_directory_after_restart',
    'zvec_storage::tests::abnormal_shutdown_recovers_committed_write',
  ]) {
    run([
      'test',
      '--locked',
      '-p',
      'fm-semantic-worker',
      '--features',
      'semantic-runtime',
      '--lib',
      testName,
    ]);
  }
  run(['test', '--locked', '-p', 'fm-semantic-components']);
} finally {
  fs.rmSync(isolatedRuntimeDirectory, { recursive: true, force: true });
}
