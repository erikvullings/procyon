import { createHash } from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { semanticCatalogSourceRevision } from './create-semantic-component-release-manifest.mjs';
import { validateRiskAcceptedGemmaApproval } from './gemma-risk-acceptance-contract.mjs';

const defaultManifestPath = fileURLToPath(
  new URL('../docs/evaluations/semantic-component-release-v1.json', import.meta.url),
);
const supportedTargets = ['linux-aarch64', 'linux-x86_64', 'macos-aarch64', 'windows-x86_64'];
const qualifiedGemmaGates = [
  'resourceCeilings',
  'cpuFallback',
  'crossHostNavigation',
  'upgradeRollback',
  'mediaSearch',
];
const previousExperimentalGemmaSource = '4553da275e76638678c7fc0a6ddcea732a19165b';
const preIntegrationGemmaSource = 'd3f921fe08e4994a905727a71af102979f4fbe23';

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
  const gemmaModes = [
    manifest.experimentalGemma,
    manifest.nonExperimentalGemma,
    manifest.riskAcceptedGemma,
  ].filter(Boolean);
  if (gemmaModes.length > 1) {
    throw new Error(
      'experimental, qualified, and risk-accepted Gemma approval modes cannot be combined',
    );
  }
  if (manifest.riskAcceptedGemma) {
    validateRiskAcceptedGemmaApproval(manifest);
  }
  if (manifest.experimentalGemma) {
    if (
      manifest.experimentalGemma.approval !== 'explicit-opt-in-experimental' ||
      manifest.experimentalGemma.candidateDecision !== 'noGo' ||
      manifest.sourceRevision === 'd3f921fe08e4994a905727a71af102979f4fbe23' ||
      Object.keys(manifest.targets ?? {})
        .sort()
        .join(',') !==
        ['linux-aarch64', 'linux-x86_64', 'macos-aarch64', 'windows-x86_64'].join(',')
    ) {
      throw new Error(
        'a new, explicitly approved four-target experimental Gemma release is required',
      );
    }
  }
  if (manifest.nonExperimentalGemma) {
    const approval = manifest.nonExperimentalGemma;
    if (
      approval.approval !== 'release-owner-approved-five-gate-qualification' ||
      !['go', 'noGo'].includes(approval.candidateDecision) ||
      [previousExperimentalGemmaSource, preIntegrationGemmaSource].includes(
        manifest.sourceRevision,
      ) ||
      manifest.releaseTag === 'semantic-v0.5.0-gemma-experimental-r3' ||
      !/^[0-9a-f]{40}$/u.test(manifest.sourceRevision ?? '') ||
      !/^[0-9a-f]{40}$/u.test(approval.modelRevision ?? '') ||
      !/^[0-9a-f]{64}$/u.test(approval.candidateManifestSha256 ?? '') ||
      typeof approval.approvedBy !== 'string' ||
      approval.approvedBy.trim().length === 0 ||
      typeof approval.approvedAt !== 'string' ||
      !/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?Z$/u.test(approval.approvedAt) ||
      !Number.isFinite(Date.parse(approval.approvedAt)) ||
      Object.keys(approval.gates ?? {})
        .sort()
        .join(',') !== [...qualifiedGemmaGates].sort().join(',') ||
      qualifiedGemmaGates.some((gate) => approval.gates[gate]?.result !== 'Pass') ||
      Object.keys(manifest.targets ?? {})
        .sort()
        .join(',') !== [...supportedTargets].sort().join(',')
    ) {
      throw new Error('a new, release-owner-approved five-gate Gemma release is required');
    }
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
    repository,
    sourceRevision,
    releaseTag,
    catalogRevision: requireString(targetEvidence.catalogRevision, 'catalogRevision'),
    catalogSha256: requireString(targetEvidence.catalogSha256, 'catalogSha256'),
    signatureSha256: requireString(targetEvidence.signatureSha256, 'signatureSha256'),
    ...(manifest.experimentalGemma || manifest.nonExperimentalGemma || manifest.riskAcceptedGemma
      ? {
          gemmaModelRevision: requireString(
            manifest.experimentalGemma?.modelRevision ??
              manifest.nonExperimentalGemma?.modelRevision ??
              manifest.riskAcceptedGemma?.modelRevision,
            manifest.experimentalGemma
              ? 'experimental Gemma model revision'
              : manifest.nonExperimentalGemma
                ? 'qualified Gemma model revision'
                : 'risk-accepted Gemma model revision',
          ),
        }
      : {}),
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
  if (plan.gemmaModelRevision) {
    const models = envelope.catalog?.models?.filter(
      (model) => model.metadata?.identity?.model === 'google-embeddinggemma-2',
    );
    if (models?.length !== 1 || models[0].metadata.identity.revision !== plan.gemmaModelRevision) {
      throw new Error('semantic component Gemma model identity differs from approval');
    }
  }
  if (envelope.catalog?.revision !== plan.catalogRevision) {
    throw new Error(`semantic component ${plan.target} catalog revision does not match approval`);
  }
  if (semanticCatalogSourceRevision(envelope, plan.repository) !== plan.sourceRevision) {
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
