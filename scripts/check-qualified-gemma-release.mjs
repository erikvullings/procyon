import { createHash } from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { verifySemanticWorkerSource } from './check-desktop-gemma-release.mjs';
import { verifySemanticComponentRelease } from './verify-semantic-component-release.mjs';

export const qualifiedGemmaManifestPath = fileURLToPath(
  new URL('../docs/evaluations/semantic-gemma-qualified-v1.json', import.meta.url),
);

const rejectedCandidateSources = new Set([
  '4553da275e76638678c7fc0a6ddcea732a19165b',
  'd3f921fe08e4994a905727a71af102979f4fbe23',
]);
const oldExperimentalTag = 'semantic-v0.5.0-gemma-experimental-r3';
const targets = ['linux-aarch64', 'linux-x86_64', 'macos-aarch64', 'windows-x86_64'];
const gates = [
  'resourceCeilings',
  'cpuFallback',
  'crossHostNavigation',
  'upgradeRollback',
  'mediaSearch',
];
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

function sha256(bytes) {
  return createHash('sha256').update(bytes).digest('hex');
}

function validateApproval(manifest) {
  const approval = manifest?.nonExperimentalGemma;
  if (
    manifest?.schemaVersion !== 1 ||
    manifest?.decision !== 'go' ||
    manifest?.repository !== 'erikvullings/procyon' ||
    approval?.approval !== 'release-owner-approved-five-gate-qualification' ||
    !['go', 'noGo'].includes(approval?.candidateDecision) ||
    !/^[0-9a-f]{40}$/u.test(manifest.sourceRevision ?? '') ||
    rejectedCandidateSources.has(manifest.sourceRevision) ||
    manifest.releaseTag === oldExperimentalTag ||
    !/^[0-9a-f]{40}$/u.test(approval.modelRevision ?? '') ||
    !/^[0-9a-f]{64}$/u.test(approval.candidateManifestSha256 ?? '') ||
    typeof approval.approvedBy !== 'string' ||
    approval.approvedBy.trim().length === 0 ||
    typeof approval.approvedAt !== 'string' ||
    !/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?Z$/u.test(approval.approvedAt) ||
    !Number.isFinite(Date.parse(approval.approvedAt)) ||
    Object.keys(approval.gates ?? {})
      .sort()
      .join(',') !== [...gates].sort().join(',') ||
    gates.some((gate) => approval.gates[gate]?.result !== 'Pass')
  ) {
    throw new Error('Gemma requires an independently recorded release-owner five-gate approval');
  }
  if (
    Object.keys(manifest.targets ?? {})
      .sort()
      .join(',') !== [...targets].sort().join(',') ||
    targets.some(
      (target) =>
        !/^[0-9a-f]{64}$/u.test(manifest.targets[target]?.catalogSha256 ?? '') ||
        !/^[0-9a-f]{64}$/u.test(manifest.targets[target]?.signatureSha256 ?? '') ||
        typeof manifest.targets[target]?.catalogRevision !== 'string' ||
        manifest.targets[target].catalogRevision.length === 0,
    )
  ) {
    throw new Error('qualified Gemma approval requires exact signed catalogs for all four targets');
  }
  return approval;
}

export function checkQualifiedGemmaDesktopRelease(
  manifest,
  semanticEnabled,
  qualifiedEnabled,
  macosArchitecture = 'universal',
  releaseSourceRevision,
  repositoryRoot = process.cwd(),
) {
  if (semanticEnabled !== 'true' || qualifiedEnabled !== 'true') {
    throw new Error('non-experimental Gemma desktop requires both independent release approvals');
  }
  validateApproval(manifest);
  if (Object.keys(manifest.targets ?? {}).length !== targets.length) {
    throw new Error('qualified Gemma requires an approved four-target component release');
  }
  if (macosArchitecture !== 'arm64') {
    throw new Error('qualified Gemma desktop requires an arm64-only macOS build');
  }
  if (!/^[0-9a-f]{40}$/u.test(releaseSourceRevision ?? '')) {
    throw new Error('desktop release source must be a 40-character commit');
  }
  verifySemanticWorkerSource(manifest.sourceRevision, releaseSourceRevision, repositoryRoot);
  return manifest;
}

export function checkQualifiedGemmaRelease(
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
  const approvalDetails = validateApproval(approval);
  if (
    !Number.isSafeInteger(expectedRunId) ||
    expectedRunId <= 0 ||
    !/^semantic-v[0-9][0-9A-Za-z.-]*$/u.test(expectedReleaseTag) ||
    rejectedCandidateSources.has(candidate.sourceRevision) ||
    candidate.releaseTag === oldExperimentalTag ||
    sha256(candidateBytes) !== approvalDetails.candidateManifestSha256 ||
    candidate.decision !== approvalDetails.candidateDecision ||
    candidate.repository !== approval.repository ||
    candidate.releaseTag !== expectedReleaseTag ||
    candidate.qualificationRunId !== expectedRunId ||
    candidate.sourceRevision !== approval.sourceRevision ||
    candidate.releaseCandidateFingerprint !== approval.releaseCandidateFingerprint ||
    !matchesCandidateTargets(candidate.targets, approval.targets) ||
    approval.releaseTag !== candidate.releaseTag ||
    approval.qualificationRunId !== candidate.qualificationRunId
  ) {
    throw new Error('qualified Gemma approval does not match the exact new signed candidate');
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
      const workers = artifacts.filter(
        (artifact) => artifact.component_id === 'procyon.semantic.worker',
      );
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
        workers.length !== 1 ||
        gemmaFiles.some((component) => gemmaArtifacts[component]?.length !== 1) ||
        model?.primary_file_name !== 'model.safetensors' ||
        model?.artifact_id !== gemmaArtifacts[gemmaFiles[0]]?.[0]?.id ||
        !modelFilesMatch
      ) {
        throw new Error('signed Gemma catalog differs from the five-gate approval');
      }
    },
  );
  return { releaseTag: candidate.releaseTag, sourceRevision: candidate.sourceRevision };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  if (!fs.existsSync(qualifiedGemmaManifestPath)) {
    throw new Error(
      'qualified Gemma approval record is missing; five-gate qualification has not been recorded',
    );
  }
  const manifest = JSON.parse(fs.readFileSync(qualifiedGemmaManifestPath, 'utf8'));
  if (process.argv.includes('--desktop')) {
    const releaseSourceRevisionIndex = process.argv.indexOf('--release-source-revision');
    const releaseSourceRevision =
      releaseSourceRevisionIndex === -1 ? undefined : process.argv[releaseSourceRevisionIndex + 1];
    if (
      releaseSourceRevisionIndex === -1 ||
      !releaseSourceRevision ||
      !/^[0-9a-f]{40}$/u.test(releaseSourceRevision)
    ) {
      throw new Error('desktop release source must be a 40-character commit');
    }
    checkQualifiedGemmaDesktopRelease(
      manifest,
      process.env.SEMANTIC_RELEASE_QUALIFIED,
      process.env.SEMANTIC_GEMMA_NON_EXPERIMENTAL_APPROVED,
      process.argv.includes('--macos-arm64-only') ? 'arm64' : 'universal',
      releaseSourceRevision,
    );
    console.log(
      `Qualified Gemma desktop uses reviewed signed candidate ${manifest.qualificationRunId}`,
    );
  } else {
    const [, , assetsRoot, candidateRoot, runId, releaseTag] = process.argv;
    if (!assetsRoot || !candidateRoot || !runId || !releaseTag) {
      throw new Error(
        'Usage: check-qualified-gemma-release.mjs <assets> <candidate-lock> <run-id> <release-tag>',
      );
    }
    const expectedRunId = Number(runId);
    if (!Number.isSafeInteger(expectedRunId) || expectedRunId <= 0) {
      throw new Error('qualification run ID must be a positive integer');
    }
    console.log(
      JSON.stringify(
        checkQualifiedGemmaRelease(assetsRoot, candidateRoot, manifest, expectedRunId, releaseTag),
      ),
    );
  }
}
