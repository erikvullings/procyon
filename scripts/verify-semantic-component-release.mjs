import { createHash } from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import {
  semanticComponentReleasePlan,
  verifySemanticCatalogBytes,
} from './fetch-approved-semantic-catalog.mjs';

const supportedTargets = ['linux-aarch64', 'linux-x86_64', 'macos-aarch64', 'windows-x86_64'];

export function verifySemanticPayloads(envelope, assetsRoot, target) {
  const artifacts = envelope.catalog?.artifacts;
  if (!Array.isArray(artifacts) || artifacts.length === 0) {
    throw new Error(`semantic component ${target} catalog has no payloads`);
  }
  for (const artifact of artifacts) {
    if (
      typeof artifact.id !== 'string' ||
      artifact.id.length === 0 ||
      path.posix.basename(artifact.id) !== artifact.id ||
      path.win32.basename(artifact.id) !== artifact.id
    ) {
      throw new Error(`semantic component ${target} catalog has an invalid payload ID`);
    }
    const payload = fs.readFileSync(path.join(assetsRoot, artifact.id));
    const expectedSha256 = Buffer.from(artifact.checksum ?? []).toString('hex');
    const actualSha256 = createHash('sha256').update(payload).digest('hex');
    if (actualSha256 !== expectedSha256) {
      throw new Error(`semantic component payload ${artifact.id} SHA-256 does not match catalog`);
    }
    if (payload.byteLength !== artifact.resources?.download_bytes) {
      throw new Error(`semantic component payload ${artifact.id} size does not match catalog`);
    }
  }
}

export function verifySemanticComponentRelease(
  manifest,
  assetsRoot,
  qualificationRunId,
  releaseTag,
) {
  if (manifest.qualificationRunId !== qualificationRunId || manifest.releaseTag !== releaseTag) {
    throw new Error('semantic component publication does not match the approved run and tag');
  }
  for (const target of supportedTargets) {
    const plan = semanticComponentReleasePlan(manifest, target);
    const envelope = verifySemanticCatalogBytes(
      plan,
      fs.readFileSync(path.join(assetsRoot, `semantic-catalog-${target}.json`)),
      fs.readFileSync(path.join(assetsRoot, `semantic-catalog-${target}.sig`)),
    );
    verifySemanticPayloads(envelope, assetsRoot, target);
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const [, , assetsRoot, runId, releaseTag, manifestPath] = process.argv;
  if (!assetsRoot || !runId || !releaseTag || !manifestPath) {
    throw new Error(
      'Usage: verify-semantic-component-release.mjs ' +
        '<assets-root> <qualification-run-id> <release-tag> <manifest>',
    );
  }
  const qualificationRunId = Number(runId);
  if (!Number.isSafeInteger(qualificationRunId) || qualificationRunId <= 0) {
    throw new Error('qualification run ID must be a positive integer');
  }
  verifySemanticComponentRelease(
    JSON.parse(fs.readFileSync(manifestPath, 'utf8')),
    assetsRoot,
    qualificationRunId,
    releaseTag,
  );
  console.log(`Verified semantic component release ${releaseTag} from run ${runId}.`);
}
