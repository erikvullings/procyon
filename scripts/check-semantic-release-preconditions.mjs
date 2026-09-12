// Fails closed unless the checked-in task-0198 report records a recomputed,
// supported-target production go.
import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const reportArgument = process.argv.indexOf('--report');
const approvedArgument = process.argv.indexOf('--approved-report');
const reportPath =
  reportArgument >= 0
    ? process.argv[reportArgument + 1]
    : fileURLToPath(new URL('../docs/evaluations/semantic-production-v1.json', import.meta.url));
const approvedPath = approvedArgument >= 0 ? process.argv[approvedArgument + 1] : undefined;
const plan = {
  command: 'cargo',
  arguments:
    reportPath === undefined
      ? []
      : [
          'run',
          '--quiet',
          '--locked',
          '-p',
          'fm-application',
          '--example',
          'validate_semantic_release_report',
          '--',
          reportPath,
        ],
};

if (process.argv.includes('--print-plan')) {
  console.log(JSON.stringify(plan, null, 2));
  process.exit(0);
}
if (reportPath === undefined) {
  throw new Error('--report requires a path');
}
if (approvedArgument >= 0 && approvedPath === undefined) {
  throw new Error('--approved-report requires a path');
}

const reportBytes = readFileSync(reportPath, 'utf8');
const report = JSON.parse(reportBytes);
if (
  report.decision !== 'go' ||
  report.productionMeasurement !== true ||
  !Array.isArray(report.measurements) ||
  report.measurements.length !== 4 ||
  !Array.isArray(report.blockingReasons) ||
  report.blockingReasons.length !== 0
) {
  throw new Error(
    'the repository semantic evaluation is not a measured, four-target go; refusing a qualified release',
  );
}
if (
  approvedPath !== undefined &&
  JSON.stringify(report) !== JSON.stringify(JSON.parse(readFileSync(approvedPath, 'utf8')))
) {
  throw new Error(
    'the freshly evaluated semantic candidate does not exactly match the reviewed repository report',
  );
}

const validation = spawnSync(plan.command, plan.arguments, { encoding: 'utf8' });
if (validation.error) throw validation.error;
if (validation.status !== 0) {
  process.stderr.write(`${validation.stdout ?? ''}${validation.stderr ?? ''}`);
  throw new Error('the repository semantic evaluation evidence does not support a go');
}
console.log('Verified a measured semantic production go decision.');
