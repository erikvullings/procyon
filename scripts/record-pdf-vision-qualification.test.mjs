import assert from 'node:assert/strict';
import { test } from 'node:test';

import { recordPdfVisionQualification } from './record-pdf-vision-qualification.mjs';

const sourceRevision = 'a'.repeat(40);
const catalogSha256 = 'b'.repeat(64);
const measured = {
  sourceBytes: 175_530_108,
  sourceSha256: 'f2e8e12f962f43b5e384dbb8a2648c8ce88213a9895bd11bb5e6fc7b350d3964',
  pageCount: 354,
  imageEvidenceCount: 354,
  backend: 'google/embeddinggemma-2 (native CPU FP32)',
  elapsedSeconds: 12_345,
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
  assert.equal(report.pageCount, 354);
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
      recordPdfVisionQualification(measured, lifecycle, candidate, {
        ...identity,
        target: 'windows-x86_64',
      }),
    /exact signed installed candidate/u,
  );
  assert.throws(
    () =>
      recordPdfVisionQualification(
        { ...measured, imageEvidenceCount: 353 },
        lifecycle,
        candidate,
        identity,
      ),
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
