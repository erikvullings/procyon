export function gemmaInstalledLimitations(dimensions) {
  const backends = new Set(dimensions.map(({ workerComputeBackend }) => workerComputeBackend));
  if (dimensions.length !== 4 || backends.size !== 1) {
    throw new Error('installed Gemma measurements require one reported backend at all four widths');
  }
  const [backend] = backends;
  const common =
    'Single synthetic asset per modality and three text queries; fresh process and index per dimension; wall includes IPC and queueing; no quality labels, paired CPU/Metal comparison, sustained throughput, or user workload. macOS peak is sampled, not a kernel high-water mark.';
  if (backend === 'google/embeddinggemma-2 (FP32 Metal)') {
    return `FP32 Metal selected for the worker; audio preprocessing and video decoding still run on CPU. ${common}`;
  }
  if (backend.startsWith('google/embeddinggemma-2 (native CPU')) {
    return `CPU selected for the worker; no Metal measurement on this target. ${common}`;
  }
  throw new Error(`unrecognized installed Gemma compute backend: ${backend}`);
}
