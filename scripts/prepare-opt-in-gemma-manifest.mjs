import { createHash } from 'node:crypto';
import fs from 'node:fs';
import { fileURLToPath } from 'node:url';

import { optInGemmaLimitations, validateOptInGemmaApproval } from './gemma-opt-in-contract.mjs';

/**
 * Builds the opt-in Gemma manifest for a freshly qualified candidate lock
 * (`semantic-component-release.json` from the qualification run's
 * `semantic-component-candidate-<sha>` artifact). Release-owner inputs (who approved, when, the
 * pinned model revision) are explicit arguments; the result is validated before it is returned.
 */
export function prepareOptInGemmaManifest(
  candidateBytes,
  { modelRevision, approvedBy, approvedAt },
) {
  const candidate = JSON.parse(candidateBytes);
  const manifest = {
    schemaVersion: 1,
    decision: 'go',
    repository: candidate.repository,
    releaseTag: candidate.releaseTag,
    qualificationRunId: candidate.qualificationRunId,
    sourceRevision: candidate.sourceRevision,
    releaseCandidateFingerprint: candidate.releaseCandidateFingerprint,
    targets: candidate.targets,
    optInGemma: {
      approval: 'release-owner-approved-opt-in',
      candidateDecision: candidate.decision,
      qualificationStatus: 'not-five-gate-qualified',
      modelRevision,
      candidateManifestSha256: createHash('sha256').update(candidateBytes).digest('hex'),
      approvedBy,
      approvedAt,
      acceptedLimitations: [...optInGemmaLimitations],
    },
  };
  validateOptInGemmaApproval(manifest);
  return manifest;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const [, , candidatePath, modelRevision, approvedBy, approvedAt, output] = process.argv;
  if (!candidatePath || !modelRevision || !approvedBy || !approvedAt || !output) {
    throw new Error(
      'Usage: prepare-opt-in-gemma-manifest.mjs <semantic-component-release.json> ' +
        '<model-revision> <approved-by> <approved-at-utc> <output>',
    );
  }
  const manifest = prepareOptInGemmaManifest(fs.readFileSync(candidatePath), {
    modelRevision,
    approvedBy,
    approvedAt,
  });
  fs.writeFileSync(output, `${JSON.stringify(manifest, null, 2)}\n`);
}
