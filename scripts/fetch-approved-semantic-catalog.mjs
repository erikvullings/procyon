import { createHash } from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const defaultManifestPath = fileURLToPath(
  new URL('../docs/evaluations/semantic-component-release-v1.json', import.meta.url),
);

function sha256(bytes) {
  return createHash('sha256').update(bytes).digest('hex');
}

function requireString(value, name) {
  if (typeof value !== 'string' || value.length === 0) {
    throw new Error(`semantic component release ${name} is required`);
  }
  return value;
}

export function semanticComponentReleasePlan(manifest, target) {
  if (manifest?.schemaVersion !== 1 || manifest?.decision !== 'go') {
    throw new Error('an approved semantic component release is required');
  }
  const repository = requireString(manifest.repository, 'repository');
  const releaseTag = requireString(manifest.releaseTag, 'releaseTag');
  const sourceRevision = requireString(manifest.sourceRevision, 'sourceRevision');
  const targetEvidence = manifest.targets?.[target];
  if (!targetEvidence) {
    throw new Error(`approved semantic component target ${target} is missing`);
  }
  const baseUrl = `https://github.com/${repository}/releases/download/${releaseTag}`;
  return {
    target,
    sourceRevision,
    releaseTag,
    catalogRevision: requireString(targetEvidence.catalogRevision, 'catalogRevision'),
    catalogSha256: requireString(targetEvidence.catalogSha256, 'catalogSha256'),
    signatureSha256: requireString(targetEvidence.signatureSha256, 'signatureSha256'),
    catalogUrl: `${baseUrl}/semantic-catalog-${target}.json`,
    signatureUrl: `${baseUrl}/semantic-catalog-${target}.sig`,
    artifactBaseUrl: `${baseUrl}/`,
  };
}

export function verifySemanticCatalogBytes(plan, catalogBytes, signatureBytes) {
  if (sha256(catalogBytes) !== plan.catalogSha256) {
    throw new Error(`semantic component ${plan.target} catalog SHA-256 does not match approval`);
  }
  if (sha256(signatureBytes) !== plan.signatureSha256) {
    throw new Error(`semantic component ${plan.target} signature SHA-256 does not match approval`);
  }
  const envelope = JSON.parse(catalogBytes.toString('utf8'));
  if (envelope.catalog?.revision !== plan.catalogRevision) {
    throw new Error(`semantic component ${plan.target} catalog revision does not match approval`);
  }
  if (envelope.catalog?.production?.source_revision !== plan.sourceRevision) {
    throw new Error(`semantic component ${plan.target} source revision does not match approval`);
  }
  for (const artifact of envelope.catalog?.artifacts ?? []) {
    if (
      typeof artifact.location !== 'string' ||
      artifact.location !== `${plan.artifactBaseUrl}${artifact.id}`
    ) {
      throw new Error(
        `semantic component ${plan.target} artifact ${artifact.id ?? '<unknown>'} ` +
          'does not use the approved immutable release',
      );
    }
  }
  return envelope;
}

async function download(url) {
  const response = await fetch(url, {
    headers: { 'user-agent': 'procyon-release-workflow' },
    redirect: 'follow',
  });
  if (!response.ok) {
    throw new Error(`failed to download ${url}: HTTP ${response.status}`);
  }
  return Buffer.from(await response.arrayBuffer());
}

async function main() {
  const [, , target, outputDirectory, manifestPath = defaultManifestPath] = process.argv;
  if (!target || !outputDirectory) {
    throw new Error(
      'Usage: fetch-approved-semantic-catalog.mjs <target> <output-directory> [manifest]',
    );
  }
  const manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8'));
  const plan = semanticComponentReleasePlan(manifest, target);
  const [catalogBytes, signatureBytes] = await Promise.all([
    download(plan.catalogUrl),
    download(plan.signatureUrl),
  ]);
  verifySemanticCatalogBytes(plan, catalogBytes, signatureBytes);
  fs.mkdirSync(outputDirectory, { recursive: true });
  fs.writeFileSync(path.join(outputDirectory, 'catalog.json'), catalogBytes);
  fs.writeFileSync(path.join(outputDirectory, 'catalog.sig'), signatureBytes);
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  await main();
}
