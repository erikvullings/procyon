import fs from 'node:fs';
import { fileURLToPath } from 'node:url';

const sourceHash = 'f2e8e12f962f43b5e384dbb8a2648c8ce88213a9895bd11bb5e6fc7b350d3964';

export function recordPdfVisionQualification(measured, lifecycle, candidate, identity) {
  const { target, qualificationRunId, qualificationSourceRevision, candidateRunId } = identity;
  const approvedTarget = candidate.targets?.[target];
  if (
    !approvedTarget ||
    !/^[0-9a-f]{40}$/u.test(candidate.sourceRevision ?? '') ||
    candidate.sourceRevision !== qualificationSourceRevision ||
    candidate.qualificationRunId !== Number(candidateRunId) ||
    lifecycle.target !== target ||
    lifecycle.catalogSha256 !== approvedTarget.catalogSha256 ||
    measured.sourceBytes !== 175_530_108 ||
    measured.sourceSha256 !== sourceHash ||
    measured.pageCount !== 354 ||
    measured.imageEvidenceCount !== 354 ||
    !Number.isFinite(measured.elapsedSeconds) ||
    measured.elapsedSeconds <= 0 ||
    typeof measured.backend !== 'string' ||
    !measured.backend.startsWith('google/embeddinggemma-2 (')
  ) {
    throw new Error('PDF vision evidence does not match the exact signed installed candidate');
  }
  return {
    schemaVersion: 1,
    decision: 'noGo',
    scope: 'installed-worker-ipc-synthetic-pdf-vision',
    target,
    qualificationRunId,
    qualificationSourceRevision,
    candidateRunId: Number(candidateRunId),
    candidateSourceRevision: candidate.sourceRevision,
    catalogSha256: lifecycle.catalogSha256,
    sourceBytes: measured.sourceBytes,
    sourceSha256: measured.sourceSha256,
    pageCount: measured.pageCount,
    imageEvidenceCount: measured.imageEvidenceCount,
    backend: measured.backend,
    elapsedSeconds: measured.elapsedSeconds,
    resourceCeilingQualified: false,
    workerPeakRssBytes: null,
    temporaryDiskPeakBytes: null,
    limitations:
      'Signed installed worker/IPC synthetic fixture only; no host VFS, peak memory or temporary disk measurement, CPU fallback, or query-quality approval.',
  };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const [, , measuredPath, lifecyclePath, candidatePath, output] = process.argv;
  if (!measuredPath || !lifecyclePath || !candidatePath || !output) {
    throw new Error(
      'Usage: record-pdf-vision-qualification.mjs <measured> <lifecycle> <candidate> <output>',
    );
  }
  const report = recordPdfVisionQualification(
    JSON.parse(fs.readFileSync(measuredPath, 'utf8')),
    JSON.parse(fs.readFileSync(lifecyclePath, 'utf8')),
    JSON.parse(fs.readFileSync(candidatePath, 'utf8')),
    {
      target: process.env.GEMMA_TARGET,
      qualificationRunId: process.env.GITHUB_RUN_ID,
      qualificationSourceRevision: process.env.GITHUB_SHA,
      candidateRunId: process.env.CANDIDATE_RUN_ID,
    },
  );
  fs.writeFileSync(output, `${JSON.stringify(report, null, 2)}\n`);
}
