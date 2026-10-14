import assert from 'node:assert/strict';
import { test } from 'node:test';

import {
  recordPdfHostQualification,
  recordPdfVisionQualification,
} from './record-pdf-vision-qualification.mjs';

const sourceRevision = 'a'.repeat(40);
const catalogSha256 = 'b'.repeat(64);
const measured = {
  sourceBytes: 3_967_050,
  sourceSha256: '6fd090eee5fcdd7e547c8787f8ada4465e896fcff190b84e2b2f49e9c5b3d690',
  pageCount: 8,
  imageEvidenceCount: 4,
  pdfImageSkippedPages: 4,
  completeGenerationVerified: true,
  backend: 'google/embeddinggemma-2 (native CPU FP32)',
  elapsedSeconds: 240,
  secondsPerPage: 30,
  pagesPerSecond: 1 / 30,
  secondsPerImagePage: 60,
  imagePagesPerSecond: 1 / 60,
  workerPeakRssBytes: 1_234_567_890,
  workerPeakMethod: 'kernel VmHWM for worker process',
  workerDataDiskPeakBytes: 234_567_890,
  workerDataDiskPeakMethod:
    '1s linked worker-data directory samples; anonymous PDF spools and transient peaks are excluded',
};
const lifecycle = { target: 'linux-aarch64', catalogSha256 };
const candidate = {
  sourceRevision,
  qualificationRunId: 100,
  targets: { 'linux-aarch64': { catalogSha256 } },
};
const identity = {
  target: 'linux-aarch64',
  qualificationRunId: '200',
  qualificationSourceRevision: sourceRevision,
  candidateRunId: '100',
};

test('a complete installed synthetic report stays no-go without measured resource ceilings', () => {
  const report = recordPdfVisionQualification(measured, lifecycle, candidate, identity);
  assert.equal(report.decision, 'noGo');
  assert.equal(report.candidateRunId, 100);
  assert.equal(report.catalogSha256, catalogSha256);
  assert.equal(report.pageCount, 8);
  assert.equal(report.imageEvidenceCount, 4);
  assert.equal(report.pdfImageSkippedPages, 4);
  assert.equal(report.completeGenerationVerified, true);
  assert.equal(report.secondsPerPage, 30);
  assert.equal(report.pagesPerSecond, 1 / 30);
  assert.equal(report.resourceCeilingQualified, false);
  assert.equal(report.workerPeakRssBytes, measured.workerPeakRssBytes);
  assert.equal(report.workerDataDiskPeakBytes, measured.workerDataDiskPeakBytes);
  assert.equal(report.temporaryDiskPeakBytes, null);
  assert.equal(report.hostPeakRssBytes, null);
});

test('installed evidence cannot be bound to a different source, target or incomplete scan', () => {
  assert.throws(
    () =>
      recordPdfVisionQualification(measured, lifecycle, candidate, {
        ...identity,
        qualificationSourceRevision: 'c'.repeat(40),
      }),
    /exact signed installed candidate/u,
  );
  assert.throws(
    () =>
      recordPdfVisionQualification(
        { ...measured, secondsPerPage: 31 },
        lifecycle,
        candidate,
        identity,
      ),
    /exact signed installed candidate/u,
  );
  assert.throws(
    () =>
      recordPdfVisionQualification(measured, lifecycle, candidate, {
        ...identity,
        target: 'windows-x86_64',
      }),
    /exact signed installed candidate/u,
  );
  assert.throws(
    () =>
      recordPdfVisionQualification(
        { ...measured, imageEvidenceCount: 7 },
        lifecycle,
        candidate,
        identity,
      ),
    /exact signed installed candidate/u,
  );
  assert.throws(
    () =>
      recordPdfVisionQualification({ ...measured, pageCount: 4 }, lifecycle, candidate, identity),
    /exact signed installed candidate/u,
  );
  assert.throws(
    () =>
      recordPdfVisionQualification(
        { ...measured, workerPeakRssBytes: 0 },
        lifecycle,
        candidate,
        identity,
      ),
    /exact signed installed candidate/u,
  );
  assert.throws(
    () =>
      recordPdfVisionQualification(
        { ...measured, workerDataDiskPeakBytes: 0 },
        lifecycle,
        candidate,
        identity,
      ),
    /exact signed installed candidate/u,
  );
});

test('Metal samples index all pages while CPU fallback is bound to the four-page cap', () => {
  const metal = recordPdfVisionQualification(
    {
      ...measured,
      imageEvidenceCount: 8,
      pdfImageSkippedPages: 0,
      backend: 'google/embeddinggemma-2 (FP32 Metal)',
      secondsPerImagePage: 30,
      imagePagesPerSecond: 1 / 30,
    },
    lifecycle,
    candidate,
    identity,
  );
  assert.equal(metal.imageEvidenceCount, 8);
  assert.equal(metal.pdfImageSkippedPages, 0);

  assert.throws(
    () =>
      recordPdfVisionQualification(
        { ...measured, imageEvidenceCount: 8, pdfImageSkippedPages: 0 },
        lifecycle,
        candidate,
        identity,
      ),
    /exact signed installed candidate/u,
  );
});

test('sampled enrolled-host PDF evidence includes replacement and deletion and remains no-go', () => {
  const sampledHost = {
    scope: 'installed-host-vfs-8-page-synthetic-pdf-sample',
    pageCount: 8,
    imageEvidenceCount: 4,
    pdfImageSkippedPages: 4,
    backend: 'google/embeddinggemma-2 (native CPU FP32)',
    completeGenerationVerified: true,
    sourceBytes: measured.sourceBytes,
    sourceSha256: measured.sourceSha256,
    elapsedSeconds: 300,
    secondsPerPage: 37.5,
    pagesPerSecond: 1 / 37.5,
    secondsPerImagePage: 75,
    imagePagesPerSecond: 1 / 75,
    hostPeakRssBytes: 600_000_000,
    workerPeakRssBytes: 1_300_000_000,
    peakRssMethod: '200ms sysinfo host and worker RSS samples; transient peaks may be missed',
    hostVfsVerified: true,
    workspaceIsolationVerified: true,
    replacementVerified: true,
    deletionVerified: true,
    currentContentAndDeletionVerified: true,
  };
  const report = recordPdfHostQualification(sampledHost, lifecycle, candidate, identity);
  assert.equal(report.candidateSourceRevision, sourceRevision);
  assert.equal(report.catalogSha256, catalogSha256);
  assert.equal(report.decision, 'noGo');
  assert.equal(report.hostVfsVerified, true);
  assert.equal(report.deletionVerified, true);
  assert.equal(report.imageEvidenceCount, 4);
  assert.equal(report.pdfImageSkippedPages, 4);
  assert.equal(report.currentContentAndDeletionVerified, true);
  assert.equal(report.secondsPerPage, sampledHost.secondsPerPage);
  assert.equal(report.pagesPerSecond, sampledHost.pagesPerSecond);
  assert.equal(report.hostPeakRssBytes, sampledHost.hostPeakRssBytes);
  assert.equal(report.workerPeakRssBytes, sampledHost.workerPeakRssBytes);
  assert.equal(report.resourceCeilingQualified, false);
  assert.throws(
    () =>
      recordPdfHostQualification(
        { ...sampledHost, imageEvidenceCount: 3 },
        lifecycle,
        candidate,
        identity,
      ),
    /exact signed installed candidate/u,
  );
  assert.throws(
    () =>
      recordPdfHostQualification(sampledHost, lifecycle, candidate, {
        ...identity,
        qualificationSourceRevision: 'c'.repeat(40),
      }),
    /exact signed installed candidate/u,
  );
  assert.throws(
    () =>
      recordPdfHostQualification(
        { ...sampledHost, hostPeakRssBytes: 0 },
        lifecycle,
        candidate,
        identity,
      ),
    /exact signed installed candidate/u,
  );
  assert.throws(
    () =>
      recordPdfHostQualification(
        { ...sampledHost, deletionVerified: false },
        lifecycle,
        candidate,
        identity,
      ),
    /exact signed installed candidate/u,
  );
  assert.throws(
    () =>
      recordPdfHostQualification(
        { ...sampledHost, currentContentAndDeletionVerified: false },
        lifecycle,
        candidate,
        identity,
      ),
    /exact signed installed candidate/u,
  );
});
