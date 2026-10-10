import { createHash } from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { verifySemanticComponentRelease } from './verify-semantic-component-release.mjs';

const targets = ['linux-aarch64', 'linux-x86_64', 'macos-aarch64', 'windows-x86_64'];
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

export function checkExperimentalGemmaRelease(assetsRoot, candidateRoot, approval) {
  const candidateBytes = fs.readFileSync(
    path.join(candidateRoot, 'semantic-component-release.json'),
  );
  const candidate = JSON.parse(candidateBytes);
  const reportBytes = fs.readFileSync(path.join(assetsRoot, 'semantic-production-evaluation.json'));
  const report = JSON.parse(reportBytes);
  const consent = approval.experimentalGemma;
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
  const [, , assetsRoot, candidateRoot, approvalPath] = process.argv;
  if (!assetsRoot || !candidateRoot || !approvalPath) {
    throw new Error(
      'Usage: check-experimental-gemma-release.mjs <assets> <candidate-lock> <approval>',
    );
  }
  console.log(
    JSON.stringify(
      checkExperimentalGemmaRelease(
        assetsRoot,
        candidateRoot,
        JSON.parse(fs.readFileSync(approvalPath, 'utf8')),
      ),
    ),
  );
}
