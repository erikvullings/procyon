import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import { fileURLToPath } from 'node:url';

import { semanticWorkerChanges } from './check-desktop-gemma-release.mjs';
import { optInGemmaManifestPath } from './gemma-opt-in.mjs';
import { autoQualificationTag } from './semantic-release-tags.mjs';

/**
 * Decides whether a push to main needs a fresh semantic qualification: only when a worker build
 * input changed since the committed signed candidate, i.e. exactly when the desktop gate
 * (`verifySemanticWorkerSource`) would refuse a release from that revision. The same
 * `semanticWorkerChanges` function decides both, so the trigger can never be looser than the gate.
 */
export function planSemanticQualification({ manifest, revision, runNumber, changesBetween }) {
  const changes = changesBetween(manifest.sourceRevision, revision);
  const candidate = manifest.sourceRevision.slice(0, 8);
  if (changes.length === 0) {
    return {
      qualify: false,
      releaseTag: '',
      reason: `worker build inputs unchanged since signed candidate ${candidate}`,
      changes,
    };
  }
  return {
    qualify: true,
    releaseTag: autoQualificationTag(manifest.releaseTag, runNumber),
    reason: `worker build inputs changed since signed candidate ${candidate}`,
    changes,
  };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const cwd = process.cwd();
  const revision =
    process.env.GITHUB_SHA ??
    execFileSync('git', ['rev-parse', 'HEAD'], { encoding: 'utf8' }).trim();
  const plan = planSemanticQualification({
    manifest: JSON.parse(fs.readFileSync(optInGemmaManifestPath, 'utf8')),
    revision,
    runNumber: Number(process.env.GITHUB_RUN_NUMBER),
    changesBetween: (from, to) => semanticWorkerChanges(from, to, cwd),
  });
  console.log(`qualify=${plan.qualify}: ${plan.reason}`);
  for (const change of plan.changes) console.log(`  ${change}`);
  if (process.env.GITHUB_OUTPUT) {
    fs.appendFileSync(
      process.env.GITHUB_OUTPUT,
      `qualify=${plan.qualify}\nrelease_tag=${plan.releaseTag}\n`,
    );
  }
}
