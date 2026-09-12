import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export function semanticEvaluationAggregationPlan(payloadRoot, output, approvedReport) {
  const reports = fs
    .readdirSync(payloadRoot, { withFileTypes: true })
    .filter((entry) => entry.isDirectory() && entry.name.startsWith('semantic-payloads-'))
    .map((entry) => path.join(payloadRoot, entry.name, 'semantic-production-evaluation.json'))
    .sort();
  if (reports.length !== 4 || reports.some((report) => !fs.statSync(report).isFile())) {
    throw new Error('exactly four supported-target semantic evaluation reports are required');
  }
  return {
    command: 'cargo',
    arguments: [
      'run',
      '--quiet',
      '--locked',
      '-p',
      'fm-application',
      '--example',
      'aggregate_semantic_production_evaluation',
      '--',
      path.resolve(output),
      path.resolve(approvedReport),
      ...reports.map((report) => path.resolve(report)),
    ],
  };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const [, , payloadRoot, output, approvedReport] = process.argv;
  if (!payloadRoot || !output || !approvedReport) {
    throw new Error(
      'Usage: aggregate-semantic-production-evaluation.mjs ' +
        '<payload-root> <output-report> <reviewed-report>',
    );
  }
  const plan = semanticEvaluationAggregationPlan(payloadRoot, output, approvedReport);
  const result = spawnSync(plan.command, plan.arguments, {
    encoding: 'utf8',
    stdio: 'inherit',
  });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    throw new Error(
      `semantic evaluation aggregation failed with status ${result.status ?? 'unknown'}`,
    );
  }
}
