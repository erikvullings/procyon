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
  for (const jobName of ['macos', 'linux', 'windows']) {
    const job = workflow.jobs?.[jobName];
    if (!excludesWorkflowDispatch(job?.if)) {
      failures.push(`${jobName}: desktop installer job must stay disabled for workflow_dispatch`);
    }
    for (const stepName of [
      `Embed the signed ${jobName === 'macos' ? 'macOS arm64' : jobName === 'linux' ? 'Linux x86-64' : 'Windows x86-64'} semantic catalog`,
      'Compile the production semantic catalog trust key',
    ]) {
      const step = job?.steps?.find((candidate) => candidate.name === stepName);
      if (!step || !excludesWorkflowDispatch(step.if)) {
        failures.push(`${jobName}: ${stepName} must stay disabled for workflow_dispatch`);
      }
    }
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

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const proof = checkSemanticQualificationWorkflow(process.argv[2]);
  console.log(JSON.stringify(proof));
}
