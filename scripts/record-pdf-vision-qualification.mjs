import fs from 'node:fs';
import { fileURLToPath } from 'node:url';

const samplePages = 8;
const sourceBytes = 3_967_050;
const sourceHash = '6fd090eee5fcdd7e547c8787f8ada4465e896fcff190b84e2b2f49e9c5b3d690';

function hasMeasuredThroughput(measured) {
  return (
    Number.isFinite(measured.elapsedSeconds) &&
    measured.elapsedSeconds > 0 &&
    measured.secondsPerPage === measured.elapsedSeconds / samplePages &&
    measured.pagesPerSecond === samplePages / measured.elapsedSeconds
  );
}

function hasExpectedImageCoverage(measured) {
  const backend = measured.backend;
  if (typeof backend !== 'string') return false;
  const isCpu = backend.includes('CPU');
  const isMetal = backend.includes('Metal');
  if (isCpu === isMetal) return false;
  const expectedImages = isCpu ? 4 : samplePages;
  const expectedSkipped = samplePages - expectedImages;
  return (
    measured.imageEvidenceCount === expectedImages &&
    measured.pdfImageSkippedPages === expectedSkipped
  );
}

function hasMeasuredImageThroughput(measured) {
  return (
    Number.isFinite(measured.secondsPerImagePage) &&
    measured.secondsPerImagePage === measured.elapsedSeconds / measured.imageEvidenceCount &&
    measured.imagePagesPerSecond === measured.imageEvidenceCount / measured.elapsedSeconds
  );
}

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
    measured.sourceBytes !== sourceBytes ||
    measured.sourceSha256 !== sourceHash ||
    measured.pageCount !== samplePages ||
    !hasExpectedImageCoverage(measured) ||
    measured.completeGenerationVerified !== true ||
    !hasMeasuredThroughput(measured) ||
    !hasMeasuredImageThroughput(measured) ||
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
    scope: 'installed-worker-ipc-8-page-synthetic-pdf-sample',
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
    pdfImageSkippedPages: measured.pdfImageSkippedPages,
    completeGenerationVerified: true,
    elapsedSeconds: measured.elapsedSeconds,
    secondsPerPage: measured.secondsPerPage,
    pagesPerSecond: measured.pagesPerSecond,
    secondsPerImagePage: measured.secondsPerImagePage,
    imagePagesPerSecond: measured.imagePagesPerSecond,
    backend: measured.backend,
    resourceCeilingQualified: false,
    hostPeakRssBytes: null,
    workerPeakRssBytes: measured.workerPeakRssBytes,
    workerPeakMethod: measured.workerPeakMethod,
    workerDataDiskPeakBytes: measured.workerDataDiskPeakBytes,
    workerDataDiskPeakMethod: measured.workerDataDiskPeakMethod,
    temporaryDiskPeakBytes: null,
    limitations:
      'Eight-page signed installed worker/IPC sample; linked worker-data sampling excludes anonymous PDF spools and transient disk peaks. Full 354-page CPU indexing is a known throughput limitation, not a qualification gate. No host memory, total temporary disk ceiling, CPU fallback, or query-quality approval.',
  };
}

export function recordPdfHostQualification(measured, lifecycle, candidate, identity) {
  exactCandidateTarget(lifecycle, candidate, identity);
  if (
    measured.scope !== 'installed-host-vfs-8-page-synthetic-pdf-sample' ||
    measured.sourceBytes !== sourceBytes ||
    measured.sourceSha256 !== sourceHash ||
    measured.pageCount !== samplePages ||
    !hasExpectedImageCoverage(measured) ||
    measured.completeGenerationVerified !== true ||
    !hasMeasuredThroughput(measured) ||
    !hasMeasuredImageThroughput(measured) ||
    !Number.isSafeInteger(measured.hostPeakRssBytes) ||
    measured.hostPeakRssBytes <= 0 ||
    !Number.isSafeInteger(measured.workerPeakRssBytes) ||
    measured.workerPeakRssBytes <= 0 ||
    typeof measured.peakRssMethod !== 'string' ||
    !measured.peakRssMethod ||
    measured.hostVfsVerified !== true ||
    measured.workspaceIsolationVerified !== true ||
    measured.replacementVerified !== true ||
    measured.deletionVerified !== true ||
    measured.currentContentAndDeletionVerified !== true
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
    pdfImageSkippedPages: measured.pdfImageSkippedPages,
    backend: measured.backend,
    completeGenerationVerified: true,
    elapsedSeconds: measured.elapsedSeconds,
    secondsPerPage: measured.secondsPerPage,
    pagesPerSecond: measured.pagesPerSecond,
    secondsPerImagePage: measured.secondsPerImagePage,
    imagePagesPerSecond: measured.imagePagesPerSecond,
    hostVfsVerified: true,
    workspaceIsolationVerified: true,
    replacementVerified: true,
    deletionVerified: true,
    currentContentAndDeletionVerified: true,
    resourceCeilingQualified: false,
    hostPeakRssBytes: measured.hostPeakRssBytes,
    workerPeakRssBytes: measured.workerPeakRssBytes,
    peakRssMethod: measured.peakRssMethod,
    temporaryDiskPeakBytes: null,
    limitations:
      'Eight-page installed Tauri application-service sample verifies replacement and deletion. Sampled RSS can miss transient peaks; anonymous temporary-disk peak, full 354-page CPU throughput, cross-host navigation, upgrade/rollback and CPU fallback remain unqualified.',
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
