import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { load as loadYaml } from 'js-yaml';

const repositoryRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const defaultWorkflow = path.join(
  repositoryRoot,
  '.github',
  'workflows',
  'release-semantic-components.yml',
);

function publishingStep(step) {
  const action = String(step.uses ?? '');
  const command = String(step.run ?? '');
  return (
    action.startsWith('softprops/action-gh-release@') ||
    /\b(?:git(?:\s+-C\s+\S+)?\s+push|gh\s+release\s+(?:create|upload)|choco\s+push)\b/u.test(
      command,
    )
  );
}

export function checkSemanticQualificationWorkflow(workflowPath = defaultWorkflow) {
  const workflow = loadYaml(fs.readFileSync(workflowPath, 'utf8'));
  const failures = [];
  if (workflow.on?.push !== undefined || workflow.on?.workflow_dispatch === undefined) {
    failures.push('semantic component releases must be manual workflow_dispatch runs only');
  }
  if (workflow.permissions?.contents !== 'read') {
    failures.push('semantic component workflow must be read-only by default');
  }

  const payloads = workflow.jobs?.['semantic-payloads'];
  const payloadGate = String(payloads?.if ?? '');
  const allSteps = Object.values(workflow.jobs ?? {}).flatMap((job) => job?.steps ?? []);
  const shellCommands = allSteps.map((step) => String(step.run ?? '')).join('\n');
  if (/\$\{\{\s*inputs\./u.test(shellCommands)) {
    failures.push('workflow inputs must reach shell commands through environment variables');
  }
  const payloadCommands = (payloads?.steps ?? []).map((step) => String(step.run ?? '')).join('\n');
  const payloadConfiguration = JSON.stringify(payloads?.steps ?? []);
  if (!payloadGate.includes("inputs.qualification_run_id == ''")) {
    failures.push('semantic payload builds must run only in qualification mode');
  }
  if (
    !payloadConfiguration.includes('inputs.release_tag') ||
    !payloadCommands.includes('semantic:bundle:production') ||
    !payloadCommands.includes('smoke-semantic-production-bundle.mjs') ||
    !payloadCommands.includes('qualify-semantic-payload.mjs')
  ) {
    failures.push(
      'qualification must build against the final tag and retain smoke/privacy evidence',
    );
  }

  const collect = workflow.jobs?.['semantic-collect'];
  if (
    !Array.isArray(collect?.needs) ||
    !collect.needs.includes('semantic-payloads') ||
    !collect.needs.includes('semantic-catalogs') ||
    !String(collect.if ?? '').includes("inputs.qualification_run_id == ''")
  ) {
    failures.push(
      'component collection must require successful qualification payloads and catalogs',
    );
  }
  const collectCommands = (collect?.steps ?? []).map((step) => String(step.run ?? '')).join('\n');
  if (
    !collectCommands.includes('aggregate-semantic-production-evaluation.mjs') ||
    !collectCommands.includes('create-semantic-component-release-manifest.mjs')
  ) {
    failures.push('qualification must emit aggregate evidence and a reviewable fingerprint lock');
  }

  const publish = workflow.jobs?.['semantic-publish'];
  const publishGate = String(publish?.if ?? '');
  const publishCommands = (publish?.steps ?? []).map((step) => String(step.run ?? '')).join('\n');
  const publishSteps = publish?.steps ?? [];
  if (
    !publishGate.includes("inputs.qualification_run_id != ''") ||
    !publishGate.includes("vars.SEMANTIC_COMPONENTS_RELEASE_QUALIFIED == 'true'") ||
    publish?.permissions?.contents !== 'write' ||
    publish?.permissions?.actions !== 'read'
  ) {
    failures.push('component publication must require an exact run and the component release gate');
  }
  if (
    !publishSteps.some(
      (step) =>
        step.uses === 'actions/download-artifact@v5' &&
        String(step.with?.['run-id'] ?? '').includes('inputs.qualification_run_id'),
    ) ||
    !publishCommands.includes('check-semantic-release-preconditions.mjs') ||
    !publishCommands.includes('--approved-report') ||
    !publishCommands.includes('verify-semantic-component-release.mjs') ||
    !publishCommands.includes('gh release create')
  ) {
    failures.push('publication must verify and publish exact retained qualification artifacts');
  }
  for (const [index, step] of publishSteps.entries()) {
    if (publishingStep(step) && !publishGate.includes("inputs.qualification_run_id != ''")) {
      failures.push(`semantic-publish step ${index + 1} can publish without an exact run`);
    }
  }

  if (failures.length > 0) {
    throw new Error(`semantic component workflow is unsafe:\n- ${failures.join('\n- ')}`);
  }
  return {
    qualificationCanPublish: false,
    publicationReusesExactRun: true,
  };
}

export function assertQualificationDispatchEnvironment({
  eventName,
  qualificationRunId,
  semanticComponentsReleaseQualified,
}) {
  if (eventName !== 'workflow_dispatch') {
    throw new Error('semantic component qualification is restricted to workflow_dispatch');
  }
  const publishMode = String(qualificationRunId ?? '').trim() !== '';
  if (publishMode && String(semanticComponentsReleaseQualified ?? '').trim() !== 'true') {
    throw new Error('SEMANTIC_COMPONENTS_RELEASE_QUALIFIED must be true for publication');
  }
  return {
    workflowDispatchOnly: true,
    publishMode,
  };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const proof =
    process.argv[2] === '--dispatch-environment'
      ? assertQualificationDispatchEnvironment({
          eventName: process.env.GITHUB_EVENT_NAME,
          qualificationRunId: process.env.QUALIFICATION_RUN_ID,
          semanticComponentsReleaseQualified:
            process.env.SEMANTIC_COMPONENTS_RELEASE_QUALIFIED_VALUE,
        })
      : checkSemanticQualificationWorkflow(process.argv[2]);
  console.log(JSON.stringify(proof));
}
