export const riskAcceptedGemmaLimitations = [
  'five-gate-qualification-not-performed',
  'metal-resource-gates-1a-and-1c-fail',
  'remaining-resource-cpu-fallback-cross-host-upgrade-rollback-and-private-media-gates-unqualified',
  'cpu-performance-not-qualified',
  'windows-gpu-not-qualified',
];

const targets = ['linux-aarch64', 'linux-x86_64', 'macos-aarch64', 'windows-x86_64'];

export function validateRiskAcceptedGemmaApproval(manifest) {
  const approval = manifest?.riskAcceptedGemma;
  if (
    manifest?.schemaVersion !== 1 ||
    manifest?.decision !== 'go' ||
    manifest?.repository !== 'erikvullings/procyon' ||
    manifest?.experimentalGemma !== undefined ||
    manifest?.nonExperimentalGemma !== undefined ||
    approval?.approval !== 'release-owner-risk-accepted-unqualified' ||
    !['go', 'noGo'].includes(approval?.candidateDecision) ||
    approval?.qualificationStatus !== 'not-five-gate-qualified' ||
    typeof manifest.releaseTag !== 'string' ||
    !/^semantic-v[0-9][0-9A-Za-z.-]*$/u.test(manifest.releaseTag) ||
    !Number.isSafeInteger(manifest.qualificationRunId) ||
    manifest.qualificationRunId <= 0 ||
    typeof manifest.releaseCandidateFingerprint !== 'string' ||
    manifest.releaseCandidateFingerprint.length === 0 ||
    !/^[0-9a-f]{40}$/u.test(manifest.sourceRevision ?? '') ||
    !/^[0-9a-f]{40}$/u.test(approval.modelRevision ?? '') ||
    !/^[0-9a-f]{64}$/u.test(approval.candidateManifestSha256 ?? '') ||
    typeof approval.approvedBy !== 'string' ||
    approval.approvedBy.trim().length === 0 ||
    typeof approval.approvedAt !== 'string' ||
    !/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?Z$/u.test(approval.approvedAt) ||
    !Number.isFinite(Date.parse(approval.approvedAt)) ||
    !Array.isArray(approval.acceptedLimitations) ||
    approval.acceptedLimitations.length !== riskAcceptedGemmaLimitations.length ||
    new Set(approval.acceptedLimitations).size !== approval.acceptedLimitations.length ||
    JSON.stringify([...approval.acceptedLimitations].sort()) !==
      JSON.stringify([...riskAcceptedGemmaLimitations].sort()) ||
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
    throw new Error(
      'Gemma risk acceptance must explicitly record the unqualified status, known limitations, and exact four-target signed candidate',
    );
  }
  return approval;
}
