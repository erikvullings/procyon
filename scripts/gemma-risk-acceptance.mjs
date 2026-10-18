import { createHash } from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { verifySemanticWorkerSource } from './check-desktop-gemma-release.mjs';
import {
  riskAcceptedGemmaLimitations,
  validateRiskAcceptedGemmaApproval,
} from './gemma-risk-acceptance-contract.mjs';
import { verifySemanticComponentRelease } from './verify-semantic-component-release.mjs';

export { riskAcceptedGemmaLimitations, validateRiskAcceptedGemmaApproval };

export const riskAcceptedGemmaManifestPath = fileURLToPath(
  new URL('../docs/evaluations/semantic-gemma-risk-accepted-v1.json', import.meta.url),
);

const targets = ['linux-aarch64', 'linux-x86_64', 'macos-aarch64', 'windows-x86_64'];
const gemmaFiles = [
  'embeddinggemma2-original',
  'embeddinggemma2-file-1',
  'embeddinggemma2-file-2',
  'embeddinggemma2-file-3',
  'embeddinggemma2-file-4',
];
const gemmaModelFiles = [
  ['embeddinggemma2-file-1', 'tokenizer.json'],
  ['embeddinggemma2-file-2', 'config.json'],
  ['embeddinggemma2-file-3', 'processor_config.json'],
  ['embeddinggemma2-file-4', 'preprocessor_config.json'],
];

function sha256(bytes) {
  return createHash('sha256').update(bytes).digest('hex');
}

function matchesCandidateTargets(candidateTargets, approvalTargets) {
  return targets.every((target) => {
    const candidate = candidateTargets?.[target];
    const approval = approvalTargets?.[target];
    return (
      candidate?.catalogRevision === approval?.catalogRevision &&
      candidate?.catalogSha256 === approval?.catalogSha256 &&
      candidate?.signatureSha256 === approval?.signatureSha256
    );
  });
}

export function checkRiskAcceptedGemmaRelease(
  assetsRoot,
  candidateRoot,
  approval,
  expectedRunId,
  expectedReleaseTag,
) {
  const candidateBytes = fs.readFileSync(
    path.join(candidateRoot, 'semantic-component-release.json'),
  );
  const candidate = JSON.parse(candidateBytes);
  const approvalDetails = validateRiskAcceptedGemmaApproval(approval);
  if (
    !Number.isSafeInteger(expectedRunId) ||
    expectedRunId <= 0 ||
    !/^semantic-v[0-9][0-9A-Za-z.-]*$/u.test(expectedReleaseTag) ||
    sha256(candidateBytes) !== approvalDetails.candidateManifestSha256 ||
    candidate.decision !== approvalDetails.candidateDecision ||
    candidate.repository !== approval.repository ||
    candidate.releaseTag !== expectedReleaseTag ||
    candidate.qualificationRunId !== expectedRunId ||
    candidate.sourceRevision !== approval.sourceRevision ||
    candidate.releaseCandidateFingerprint !== approval.releaseCandidateFingerprint ||
    Object.keys(candidate.targets ?? {})
      .sort()
      .join(',') !== [...targets].sort().join(',') ||
    !matchesCandidateTargets(candidate.targets, approval.targets) ||
    approval.releaseTag !== candidate.releaseTag ||
    approval.qualificationRunId !== candidate.qualificationRunId
  ) {
    throw new Error('Gemma risk acceptance does not match the exact new signed candidate');
  }

  verifySemanticComponentRelease(
    approval,
    assetsRoot,
    expectedRunId,
    expectedReleaseTag,
    (envelope) => {
      const models = envelope.catalog?.models?.filter(
        (model) => model.metadata?.identity?.model === 'google-embeddinggemma-2',
      );
      const artifacts = envelope.catalog?.artifacts ?? [];
      const gemmaArtifacts = Object.fromEntries(
        gemmaFiles.map((component) => [
          component,
          artifacts.filter((artifact) => artifact.component_id === component),
        ]),
      );
      const model = models?.[0];
      const expectedModelFiles = Object.fromEntries(
        gemmaModelFiles.map(([component, fileName]) => [
          fileName,
          gemmaArtifacts[component]?.[0]?.id,
        ]),
      );
      const modelFiles = model?.files ?? {};
      const modelFilesMatch =
        Object.keys(modelFiles).sort().join(',') ===
          Object.keys(expectedModelFiles).sort().join(',') &&
        Object.entries(expectedModelFiles).every(([fileName, artifactId]) => {
          return modelFiles[fileName] === artifactId;
        });
      if (
        models?.length !== 1 ||
        model?.metadata?.identity?.revision !== approvalDetails.modelRevision ||
        gemmaFiles.some((component) => gemmaArtifacts[component]?.length !== 1) ||
        model?.primary_file_name !== 'model.safetensors' ||
        model?.artifact_id !== gemmaArtifacts[gemmaFiles[0]]?.[0]?.id ||
        !modelFilesMatch
      ) {
        throw new Error('signed Gemma catalog differs from the risk-accepted candidate');
      }
    },
  );
  return { releaseTag: candidate.releaseTag, sourceRevision: candidate.sourceRevision };
}

export function checkRiskAcceptedGemmaDesktopRelease(
  manifest,
  semanticEnabled,
  riskAcceptedEnabled,
  desktopTarget,
  releaseSourceRevision,
  repositoryRoot = process.cwd(),
) {
  if (semanticEnabled !== 'true' || riskAcceptedEnabled !== 'true') {
    throw new Error(
      'risk-accepted Gemma desktop requires independent semantic and owner approvals',
    );
  }
  validateRiskAcceptedGemmaApproval(manifest);
  if (Object.keys(manifest.targets ?? {}).length !== targets.length) {
    throw new Error('risk-accepted Gemma requires an approved four-target component release');
  }
  if (!targets.includes(desktopTarget) || !manifest.targets[desktopTarget]) {
    throw new Error('risk-accepted Gemma desktop target is not in the signed component release');
  }
  if (!/^[0-9a-f]{40}$/u.test(releaseSourceRevision ?? '')) {
    throw new Error('desktop release source must be a 40-character commit');
  }

  verifySemanticWorkerSource(manifest.sourceRevision, releaseSourceRevision, repositoryRoot);
  return manifest;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  if (process.argv.includes('--desktop')) {
    const releaseSourceRevisionIndex = process.argv.indexOf('--release-source-revision');
    const releaseSourceRevision =
      releaseSourceRevisionIndex === -1 ? undefined : process.argv[releaseSourceRevisionIndex + 1];
    const targetIndex = process.argv.indexOf('--target');
    const desktopTarget = targetIndex === -1 ? undefined : process.argv[targetIndex + 1];
    if (
      releaseSourceRevisionIndex === -1 ||
      !releaseSourceRevision ||
      !/^[0-9a-f]{40}$/u.test(releaseSourceRevision) ||
      !desktopTarget
    ) {
      throw new Error('desktop release requires a target and a 40-character source commit');
    }
    checkRiskAcceptedGemmaDesktopRelease(
      JSON.parse(fs.readFileSync(riskAcceptedGemmaManifestPath, 'utf8')),
      process.env.SEMANTIC_RELEASE_QUALIFIED,
      process.env.SEMANTIC_GEMMA_RISK_ACCEPTED,
      desktopTarget,
      releaseSourceRevision,
    );
    console.log(
      `Risk-accepted Gemma desktop target ${desktopTarget} uses the exact signed candidate`,
    );
  } else {
    const [, , assetsRoot, candidateRoot, runId, releaseTag] = process.argv;
    if (!assetsRoot || !candidateRoot || !runId || !releaseTag) {
      throw new Error(
        'Usage: gemma-risk-acceptance.mjs <assets> <candidate-lock> <run-id> <release-tag>',
      );
    }
    const expectedRunId = Number(runId);
    if (!Number.isSafeInteger(expectedRunId) || expectedRunId <= 0) {
      throw new Error('qualification run ID must be a positive integer');
    }
    console.log(
      JSON.stringify(
        checkRiskAcceptedGemmaRelease(
          assetsRoot,
          candidateRoot,
          JSON.parse(fs.readFileSync(riskAcceptedGemmaManifestPath, 'utf8')),
          expectedRunId,
          releaseTag,
        ),
      ),
    );
  }
}
