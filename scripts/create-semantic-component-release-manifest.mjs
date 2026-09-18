import { createHash } from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const supportedTargets = ['linux-aarch64', 'linux-x86_64', 'macos-aarch64', 'windows-x86_64'];

function sha256(bytes) {
  return createHash('sha256').update(bytes).digest('hex');
}

export function createSemanticComponentReleaseManifest({
  assetsRoot,
  evaluation,
  repository,
  releaseTag,
  qualificationRunId,
  sourceRevision,
}) {
  const targets = {};
  for (const target of supportedTargets) {
    const catalogBytes = fs.readFileSync(path.join(assetsRoot, `semantic-catalog-${target}.json`));
    const signatureBytes = fs.readFileSync(path.join(assetsRoot, `semantic-catalog-${target}.sig`));
    const envelope = JSON.parse(catalogBytes.toString('utf8'));
    if (envelope.catalog?.production?.source_revision !== sourceRevision) {
      throw new Error(`semantic component ${target} does not match source revision`);
    }
    targets[target] = {
      catalogRevision: envelope.catalog.revision,
      catalogSha256: sha256(catalogBytes),
      signatureSha256: sha256(signatureBytes),
    };
  }
  return {
    schemaVersion: 1,
    decision: evaluation.decision,
    repository,
    releaseTag,
    qualificationRunId,
    sourceRevision,
    releaseCandidateFingerprint: evaluation.releaseCandidateFingerprint,
    targets,
  };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const [, , assetsRoot, evaluationPath, repository, releaseTag, runId, sourceRevision, output] =
    process.argv;
  const qualificationRunId = Number(runId);
  if (
    !assetsRoot ||
    !evaluationPath ||
    !repository ||
    !releaseTag ||
    !Number.isSafeInteger(qualificationRunId) ||
    qualificationRunId <= 0 ||
    !sourceRevision ||
    !output
  ) {
    throw new Error(
      'Usage: create-semantic-component-release-manifest.mjs ' +
        '<assets-root> <evaluation> <repository> <release-tag> ' +
        '<qualification-run-id> <source-revision> <output>',
    );
  }
  const manifest = createSemanticComponentReleaseManifest({
    assetsRoot,
    evaluation: JSON.parse(fs.readFileSync(evaluationPath, 'utf8')),
    repository,
    releaseTag,
    qualificationRunId,
    sourceRevision,
  });
  fs.writeFileSync(output, `${JSON.stringify(manifest, null, 2)}\n`);
  console.log(output);
}
