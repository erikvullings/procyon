import fs from 'node:fs';
import { fileURLToPath } from 'node:url';

const sourceHash = 'f2e8e12f962f43b5e384dbb8a2648c8ce88213a9895bd11bb5e6fc7b350d3964';

function exactCandidateTarget(lifecycle, candidate, identity) {
  const { target, qualificationRunId, qualificationSourceRevision, candidateRunId } = identity;
  const approvedTarget = candidate.targets?.[target];
  if (
    !approvedTarget ||
    !/^[1-9][0-9]*$/u.test(String(qualificationRunId)) ||
    !/^[0-9a-f]{40}$/u.test(candidate.sourceRevision ?? '') ||
    candidate.sourceRevision !== qualificationSourceRevision ||
    candidate.qualificationRunId !== Number(candidateRunId) ||
    lifecycle.target !== target ||
    lifecycle.catalogSha256 !== approvedTarget.catalogSha256
  ) {
    throw new Error('PDF vision evidence does not match the exact signed installed candidate');
  }
}

export function recordPdfVisionQualification(measured, lifecycle, candidate, identity) {
  exactCandidateTarget(lifecycle, candidate, identity);
  const { target, qualificationRunId, qualificationSourceRevision, candidateRunId } = identity;
  if (
    measured.sourceBytes !== 175_530_108 ||
    measured.sourceSha256 !== sourceHash ||
    measured.pageCount !== 354 ||
    measured.imageEvidenceCount !== 354 ||
    !Number.isFinite(measured.elapsedSeconds) ||
    measured.elapsedSeconds <= 0 ||
    !Number.isSafeInteger(measured.workerPeakRssBytes) ||
    measured.workerPeakRssBytes <= 0 ||
    !Number.isSafeInteger(measured.workerDataDiskPeakBytes) ||
    measured.workerDataDiskPeakBytes <= 0 ||
    typeof measured.workerPeakMethod !== 'string' ||
    !measured.workerPeakMethod ||
    typeof measured.workerDataDiskPeakMethod !== 'string' ||
    !measured.workerDataDiskPeakMethod ||
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
    hostPeakRssBytes: null,
    workerPeakRssBytes: measured.workerPeakRssBytes,
    workerPeakMethod: measured.workerPeakMethod,
    workerDataDiskPeakBytes: measured.workerDataDiskPeakBytes,
    workerDataDiskPeakMethod: measured.workerDataDiskPeakMethod,
    temporaryDiskPeakBytes: null,
    limitations:
      'Signed installed worker/IPC synthetic fixture only; linked worker-data sampling excludes anonymous PDF spools and transient disk peaks. No host memory, total temporary disk ceiling, CPU fallback, or query-quality approval.',
  };
}

export function recordPdfHostQualification(measured, lifecycle, candidate, identity) {
  exactCandidateTarget(lifecycle, candidate, identity);
  if (
    measured.scope !== 'installed-host-vfs-354-page-synthetic-pdf-probe' ||
    measured.sourceBytes !== 175_530_108 ||
    measured.sourceSha256 !== sourceHash ||
    measured.pageCount !== 354 ||
    measured.imageEvidenceCount !== 354 ||
    !Number.isSafeInteger(measured.hostPeakRssBytes) ||
    measured.hostPeakRssBytes <= 0 ||
    !Number.isSafeInteger(measured.workerPeakRssBytes) ||
    measured.workerPeakRssBytes <= 0 ||
    typeof measured.peakRssMethod !== 'string' ||
    !measured.peakRssMethod ||
    measured.hostVfsVerified !== true ||
    measured.workspaceIsolationVerified !== true ||
    measured.deletionVerified !== true ||
    measured.currentContentAndDeletionVerified !== false
  ) {
    throw new Error('PDF host evidence does not match the exact signed installed candidate');
  }
  return {
    schemaVersion: 1,
    decision: 'noGo',
    scope: measured.scope,
    target: identity.target,
    qualificationRunId: identity.qualificationRunId,
    qualificationSourceRevision: identity.qualificationSourceRevision,
    candidateRunId: Number(identity.candidateRunId),
    candidateSourceRevision: candidate.sourceRevision,
    catalogSha256: lifecycle.catalogSha256,
    sourceBytes: measured.sourceBytes,
    sourceSha256: measured.sourceSha256,
    pageCount: measured.pageCount,
    imageEvidenceCount: measured.imageEvidenceCount,
    hostVfsVerified: true,
    workspaceIsolationVerified: true,
    deletionVerified: true,
    currentContentAndDeletionVerified: false,
    resourceCeilingQualified: false,
    hostPeakRssBytes: measured.hostPeakRssBytes,
    workerPeakRssBytes: measured.workerPeakRssBytes,
    peakRssMethod: measured.peakRssMethod,
    temporaryDiskPeakBytes: null,
    limitations:
      'One installed Tauri application-service full-scan fixture verifies deletion but not source edits. Sampled RSS can miss transient peaks; anonymous temporary-disk peak, cross-host navigation, upgrade/rollback and CPU fallback remain unqualified.',
  };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const [, , measuredPath, lifecyclePath, candidatePath, output, mode] = process.argv;
  if (!measuredPath || !lifecyclePath || !candidatePath || !output || (mode && mode !== '--host')) {
    throw new Error(
      'Usage: record-pdf-vision-qualification.mjs <measured> <lifecycle> <candidate> <output> [--host]',
    );
  }
  const record = mode === '--host' ? recordPdfHostQualification : recordPdfVisionQualification;
  const report = record(
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
