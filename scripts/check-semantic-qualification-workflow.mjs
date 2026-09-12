import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { load as loadYaml } from 'js-yaml';

const repositoryRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const defaultWorkflow = path.join(repositoryRoot, '.github', 'workflows', 'release-desktop.yml');

function excludesWorkflowDispatch(condition = '') {
  return /github\.event_name\s*==\s*['"]push['"]/u.test(String(condition));
}

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
  for (const [jobName, job] of Object.entries(workflow.jobs ?? {})) {
    const jobExcludesDispatch = excludesWorkflowDispatch(job.if);
    const contentsPermission = job.permissions?.contents ?? workflow.permissions?.contents;
    if (!jobExcludesDispatch && contentsPermission === 'write') {
      failures.push(`${jobName}: workflow_dispatch can receive contents: write`);
    }
    if (String(job.uses ?? '').includes('publish-chocolatey') && !jobExcludesDispatch) {
      failures.push(`${jobName}: reusable publication workflow can run on workflow_dispatch`);
    }
    for (const [index, step] of (job.steps ?? []).entries()) {
      if (publishingStep(step) && !jobExcludesDispatch && !excludesWorkflowDispatch(step.if)) {
        failures.push(
          `${jobName} step ${index + 1} (${step.name ?? step.uses ?? 'unnamed'}) can publish on workflow_dispatch`,
        );
      }
    }
  }

  const semanticPayloads = workflow.jobs?.['semantic-payloads'];
  const semanticGate = String(semanticPayloads?.if ?? '');
  if (
    !semanticGate.includes("vars.SEMANTIC_RELEASE_QUALIFIED == 'true'") ||
    !semanticGate.includes("github.event_name == 'workflow_dispatch'")
  ) {
    failures.push(
      'semantic-payloads must allow non-published dispatches and require qualification for tag builds',
    );
  }
  const bundleStep = semanticPayloads?.steps?.find(
    (step) => step.name === 'Build verified semantic release payloads',
  );
  if (!String(bundleStep?.run ?? '').includes('https://qualification.invalid/')) {
    failures.push('workflow_dispatch catalogs must use an explicitly non-published artifact URL');
  }
  const safety = workflow.jobs?.['qualification-safety'];
  if (
    String(safety?.if ?? '') !== "github.event_name == 'workflow_dispatch'" ||
    safety?.permissions?.contents !== 'read'
  ) {
    failures.push('qualification-safety must be dispatch-only with read-only contents permission');
  }
  const safetyCommands = (safety?.steps ?? []).map((step) => String(step.run ?? '')).join('\n');
  if (!safetyCommands.includes('--dispatch-environment')) {
    failures.push('qualification-safety must prove both release gates are disabled');
  }
  if (
    !Array.isArray(semanticPayloads?.needs) ||
    !semanticPayloads.needs.includes('qualification-safety')
  ) {
    failures.push('semantic-payloads must depend on qualification-safety');
  }
  const semanticCommands = (semanticPayloads?.steps ?? [])
    .map((step) => String(step.run ?? ''))
    .join('\n');
  if (!semanticCommands.includes('qualify-semantic-payload.mjs')) {
    failures.push('semantic-payloads must run worker qualification per target');
  }
  const installedQualification = workflow.jobs?.['semantic-installed-qualification'];
  const installedGate = String(installedQualification?.if ?? '');
  if (
    !installedGate.includes("github.event_name == 'workflow_dispatch'") ||
    !installedGate.includes('always()') ||
    installedQualification?.permissions?.contents !== 'read' ||
    !Array.isArray(installedQualification?.needs) ||
    !installedQualification.needs.includes('qualification-safety')
  ) {
    failures.push(
      'semantic-installed-qualification must be dispatch-only, read-only, safety-gated, and resilient to sibling failures',
    );
  }
  const installedCommands = (installedQualification?.steps ?? [])
    .map((step) => String(step.run ?? ''))
    .join('\n');
  if (
    !installedCommands.includes('qualify-semantic-installed.mjs') ||
    !installedCommands.includes('--dispatch-environment')
  ) {
    failures.push('installed qualification must repeat safety proof before package execution');
  }
  const payloadPrecondition = semanticPayloads?.steps?.find(
    (step) => step.name === 'Verify exact-production semantic evaluation evidence',
  );
  if (
    !String(payloadPrecondition?.run ?? '').includes('check-semantic-release-preconditions.mjs') ||
    !excludesWorkflowDispatch(payloadPrecondition?.if)
  ) {
    failures.push(
      'semantic-payloads must require checked-in production evaluation only for tag publication',
    );
  }
  const smokeStep = semanticPayloads?.steps?.find(
    (step) => step.name === 'Smoke-test protocol, model activation, and component lifecycle',
  );
  if (!String(smokeStep?.run ?? '').includes('--evaluation-report')) {
    failures.push('semantic-payloads must retain an exact-production evaluation report');
  }
  for (const jobName of ['macos', 'linux', 'windows']) {
    const job = workflow.jobs?.[jobName];
    if (!excludesWorkflowDispatch(job?.if)) {
      failures.push(`${jobName}: desktop installer job must stay disabled for workflow_dispatch`);
    }
    for (const stepName of [
      'Verify exact-production semantic evaluation evidence',
      `Embed the signed ${jobName === 'macos' ? 'macOS arm64' : jobName === 'linux' ? 'Linux x86-64' : 'Windows x86-64'} semantic catalog`,
      'Compile the production semantic catalog trust key',
    ]) {
      const step = job?.steps?.find((candidate) => candidate.name === stepName);
      if (!step || !excludesWorkflowDispatch(step.if)) {
        failures.push(`${jobName}: ${stepName} must stay disabled for workflow_dispatch`);
      }
    }
  }
  const collectSteps = workflow.jobs?.['semantic-collect']?.steps ?? [];
  const aggregateStep = collectSteps.find(
    (step) => step.name === 'Aggregate private supported-target semantic evaluation',
  );
  if (!String(aggregateStep?.run ?? '').includes('aggregate-semantic-production-evaluation.mjs')) {
    failures.push('semantic-collect must retain one aggregate supported-target evaluation');
  }
  const matchIndex = collectSteps.findIndex(
    (step) => step.name === 'Match evaluated payloads to the reviewed release evidence',
  );
  const uploadIndex = collectSteps.findIndex((step) => step.uses === 'actions/upload-artifact@v4');
  const matchStep = collectSteps[matchIndex];
  const semanticPublish = workflow.jobs?.['semantic-publish'];
  if (
    matchIndex < 0 ||
    uploadIndex < 0 ||
    matchIndex >= uploadIndex ||
    !excludesWorkflowDispatch(matchStep?.if) ||
    !String(matchStep?.run ?? '').includes('check-semantic-release-preconditions.mjs') ||
    !String(matchStep?.run ?? '').includes('--approved-report') ||
    !excludesWorkflowDispatch(semanticPublish?.if) ||
    !Array.isArray(semanticPublish?.needs) ||
    !semanticPublish.needs.includes('semantic-collect')
  ) {
    failures.push(
      'semantic publication must match freshly evaluated payloads to reviewed evidence before release',
    );
  }
  if (failures.length > 0) {
    throw new Error(
      `release workflow is unsafe for qualification dispatch:\n- ${failures.join('\n- ')}`,
    );
  }
  return {
    workflowDispatchCanPublish: false,
    tagSemanticPublicationRequiresQualification: true,
  };
}

export function assertQualificationDispatchEnvironment({
  eventName,
  semanticReleaseQualified,
  knowledgeSearchReleaseQualified,
}) {
  if (eventName !== 'workflow_dispatch') {
    throw new Error('installed semantic qualification is restricted to workflow_dispatch');
  }
  for (const [name, value] of [
    ['SEMANTIC_RELEASE_QUALIFIED', semanticReleaseQualified],
    ['KNOWLEDGE_SEARCH_RELEASE_QUALIFIED', knowledgeSearchReleaseQualified],
  ]) {
    const normalized = String(value ?? '').trim();
    if (normalized !== '' && normalized !== 'false') {
      throw new Error(`${name} must be absent or false during private qualification`);
    }
  }
  return {
    workflowDispatchOnly: true,
    semanticReleaseQualified: false,
    knowledgeSearchReleaseQualified: false,
  };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const proof =
    process.argv[2] === '--dispatch-environment'
      ? assertQualificationDispatchEnvironment({
          eventName: process.env.GITHUB_EVENT_NAME,
          semanticReleaseQualified: process.env.SEMANTIC_RELEASE_QUALIFIED_VALUE,
          knowledgeSearchReleaseQualified: process.env.KNOWLEDGE_SEARCH_RELEASE_QUALIFIED_VALUE,
        })
      : checkSemanticQualificationWorkflow(process.argv[2]);
  console.log(JSON.stringify(proof));
}
