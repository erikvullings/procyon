import { createHash } from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { verifySemanticComponentRelease } from './verify-semantic-component-release.mjs';

const targets = ['linux-aarch64', 'linux-x86_64', 'macos-aarch64', 'windows-x86_64'];
const preIntegrationSource = 'd3f921fe08e4994a905727a71af102979f4fbe23';
const pdfVisionSamplePages = 8;
const pdfVisionSampleBytes = 3_967_050;
const pdfVisionSampleHash = '6fd090eee5fcdd7e547c8787f8ada4465e896fcff190b84e2b2f49e9c5b3d690';
const releaseOwnerApprovedLimitations = [
  'resource-ceilings-unqualified',
  'anonymous-pdf-spool-peak-unqualified',
  'cpu-fallback-unqualified',
  'cross-host-navigation-unqualified',
  'upgrade-rollback-unqualified',
  'model-quality-unproven',
  'cpu-pdf-image-indexing-about-106-seconds-per-page-four-page-budget-about-eight-minutes',
];
const gemmaFiles = [
  'embeddinggemma2-original',
  'embeddinggemma2-file-1',
  'embeddinggemma2-file-2',
  'embeddinggemma2-file-3',
  'embeddinggemma2-file-4',
];

function sha256(bytes) {
  return createHash('sha256').update(bytes).digest('hex');
}

function verifyPdfVisionQualification(consent, candidate, evidenceRoot) {
  const qualification = consent.pdfVisionQualification;
  if (
    !qualification ||
    !Number.isSafeInteger(qualification.runId) ||
    qualification.runId <= 0 ||
    qualification.sourceRevision !== candidate.sourceRevision ||
    Object.keys(qualification.targets ?? {}).length !== targets.length ||
    !evidenceRoot
  ) {
    throw new Error('experimental Gemma requires exact four-target PDF vision qualification');
  }
  for (const target of targets) {
    const bytes = fs.readFileSync(
      path.join(evidenceRoot, `installed-gemma-${target}`, 'gemma-pdf-host-bound-report.json'),
    );
    const report = JSON.parse(bytes);
    const isCpu = typeof report.backend === 'string' && report.backend.includes('CPU');
    const isMetal = typeof report.backend === 'string' && report.backend.includes('Metal');
    const expectedImageCount = isCpu ? 4 : isMetal ? pdfVisionSamplePages : 0;
    const expectedSkippedPages = pdfVisionSamplePages - expectedImageCount;
    if (
      sha256(bytes) !== qualification.targets[target] ||
      report.schemaVersion !== 1 ||
      report.decision !== 'noGo' ||
      report.scope !== 'installed-host-vfs-8-page-synthetic-pdf-sample' ||
      report.target !== target ||
      report.qualificationRunId !== String(qualification.runId) ||
      report.qualificationSourceRevision !== candidate.sourceRevision ||
      report.candidateRunId !== candidate.qualificationRunId ||
      report.candidateSourceRevision !== candidate.sourceRevision ||
      report.catalogSha256 !== candidate.targets[target]?.catalogSha256 ||
      report.sourceBytes !== pdfVisionSampleBytes ||
      report.sourceSha256 !== pdfVisionSampleHash ||
      report.pageCount !== pdfVisionSamplePages ||
      isCpu === isMetal ||
      !report.backend.startsWith('google/embeddinggemma-2 (') ||
      report.imageEvidenceCount !== expectedImageCount ||
      report.pdfImageSkippedPages !== expectedSkippedPages ||
      report.completeGenerationVerified !== true ||
      report.hostVfsVerified !== true ||
      report.replacementVerified !== true ||
      report.fallbackVerified === true ||
      report.upgradeRollbackVerified === true ||
      report.currentContentAndDeletionVerified !== true ||
      report.crossHostNavigationVerified === true ||
      report.resourceCeilingQualified !== false ||
      !Number.isFinite(report.elapsedSeconds) ||
      report.elapsedSeconds <= 0 ||
      report.imagePagesPerSecond !== expectedImageCount / report.elapsedSeconds ||
      !Number.isFinite(report.hostPeakRssBytes) ||
      report.hostPeakRssBytes <= 0 ||
      !Number.isFinite(report.workerPeakRssBytes) ||
      report.workerPeakRssBytes <= 0 ||
      report.temporaryDiskPeakBytes !== null
    ) {
      throw new Error(`experimental Gemma ${target} PDF vision qualification is incomplete`);
    }
  }
}

export function checkExperimentalGemmaRelease(assetsRoot, candidateRoot, approval, evidenceRoot) {
  const candidateBytes = fs.readFileSync(
    path.join(candidateRoot, 'semantic-component-release.json'),
  );
  const candidate = JSON.parse(candidateBytes);
  const reportBytes = fs.readFileSync(path.join(assetsRoot, 'semantic-production-evaluation.json'));
  const report = JSON.parse(reportBytes);
  const consent = approval.experimentalGemma;
  if (
    consent?.approvalBasis !== 'release-owner-approved-with-known-limitations' ||
    consent.modelQualityDecision !== 'noGo' ||
    JSON.stringify(consent.knownLimitations) !== JSON.stringify(releaseOwnerApprovedLimitations)
  ) {
    throw new Error('experimental Gemma release requires the exact approved limitations');
  }
  if (candidate.sourceRevision === preIntegrationSource) {
    throw new Error('pre-integration Gemma worker cannot publish without a new signed candidate');
  }
  if (
    approval.schemaVersion !== 1 ||
    approval.decision !== 'go' ||
    consent?.approval !== 'explicit-opt-in-experimental' ||
    consent.candidateDecision !== 'noGo' ||
    sha256(candidateBytes) !== consent.candidateManifestSha256 ||
    sha256(reportBytes) !== consent.evaluationSha256 ||
    candidate.decision !== 'noGo' ||
    report.decision !== 'noGo' ||
    report.evidencePolicy !== 'experimental-alpha' ||
    report.productionMeasurement !== true ||
    report.measurements?.length !== 4 ||
    !report.measurements.every(
      (entry) => entry.identity?.pipeline?.model?.model === 'intfloat.multilingual-e5-small',
    ) ||
    report.releaseCandidateFingerprint !== approval.releaseCandidateFingerprint
  ) {
    throw new Error('experimental Gemma approval does not match exact retained no-go evidence');
  }
  for (const field of [
    'repository',
    'releaseTag',
    'qualificationRunId',
    'sourceRevision',
    'releaseCandidateFingerprint',
  ]) {
    if (approval[field] !== candidate[field]) {
      throw new Error(`experimental Gemma ${field} differs from signed candidate`);
    }
  }
  if (
    Object.keys(approval.targets ?? {}).length !== targets.length ||
    Object.keys(candidate.targets ?? {}).length !== targets.length
  ) {
    throw new Error('experimental Gemma approval requires exactly four targets');
  }
  for (const target of targets) {
    if (JSON.stringify(approval.targets[target]) !== JSON.stringify(candidate.targets[target])) {
      throw new Error(`experimental Gemma ${target} catalog differs from signed candidate`);
    }
  }
  verifyPdfVisionQualification(consent, candidate, evidenceRoot);
  verifySemanticComponentRelease(
    approval,
    assetsRoot,
    approval.qualificationRunId,
    approval.releaseTag,
    (envelope) => {
      const models = envelope.catalog.models.filter(
        (model) => model.metadata?.identity?.model === 'google-embeddinggemma-2',
      );
      if (
        models.length !== 1 ||
        models[0].metadata.identity.revision !== consent.modelRevision ||
        !/^[0-9a-f]{40}$/u.test(consent.modelRevision)
      ) {
        throw new Error('signed Gemma model identity differs from experimental approval');
      }
      const model = models[0];
      const artifacts = envelope.catalog.artifacts;
      const worker = artifacts.filter(
        (artifact) => artifact.component_id === 'procyon.semantic.worker',
      );
      const rendererComponents = [
        'procyon.semantic.pdfium-runtime',
        'procyon.semantic.pdfium-notices',
      ];
      if (
        worker.length !== 1 ||
        rendererComponents.some(
          (component) =>
            artifacts.filter((artifact) => artifact.component_id === component).length !== 1,
        ) ||
        !worker[0].compatibility?.runtimes?.some(
          (runtime) =>
            runtime.component_id === rendererComponents[0] &&
            runtime.version_requirement === '=0.8086.0',
        ) ||
        rendererComponents.some((component) => {
          const artifact = artifacts.find((entry) => entry.component_id === component);
          return !(envelope.provenance ?? []).some(
            (entry) =>
              entry.artifact_id === artifact.id &&
              entry.source === 'https://github.com/bblanchon/pdfium-binaries' &&
              entry.source_revision === '5325fa6d0d9379329f10f98fdc4839b4e40f2e79',
          );
        })
      ) {
        throw new Error(
          'experimental Gemma requires signed PDF renderer and notices on every target',
        );
      }
      if (
        model.primary_file_name !== 'model.safetensors' ||
        model.artifact_id !== artifacts.find((entry) => entry.component_id === gemmaFiles[0])?.id ||
        Object.keys(model.files ?? {})
          .sort()
          .join(',') !==
          [
            'config.json',
            'preprocessor_config.json',
            'processor_config.json',
            'tokenizer.json',
          ].join(',') ||
        gemmaFiles.some(
          (component) => artifacts.filter((entry) => entry.component_id === component).length !== 1,
        ) ||
        Object.values(model.files).some(
          (id) =>
            !artifacts.some((entry) => entry.id === id && gemmaFiles.includes(entry.component_id)),
        )
      ) {
        throw new Error('signed Gemma original-file set differs from experimental approval');
      }
    },
  );
  return { releaseTag: approval.releaseTag, sourceRevision: approval.sourceRevision };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const [, , assetsRoot, candidateRoot, approvalPath, evidenceRoot] = process.argv;
  if (!assetsRoot || !candidateRoot || !approvalPath || !evidenceRoot) {
    throw new Error(
      'Usage: check-experimental-gemma-release.mjs <assets> <candidate-lock> <approval> <pdf-evidence>',
    );
  }
  console.log(
    JSON.stringify(
      checkExperimentalGemmaRelease(
        assetsRoot,
        candidateRoot,
        JSON.parse(fs.readFileSync(approvalPath, 'utf8')),
        evidenceRoot,
      ),
    ),
  );
}
