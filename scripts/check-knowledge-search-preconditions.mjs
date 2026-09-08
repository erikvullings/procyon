// Supported-platform full-text index preconditions for a qualified Structured
// Knowledge Search release (task 0208).
//
// The structured full-text route depends on the Zvec full-text schema, its
// one-way migration from a vector-only collection, and the crash/restart
// lifecycle around that migration. Those tests are behind the `zvec` feature,
// so the ordinary workspace test run does not cover them; a release that claims
// a measured go must therefore run them on the platform it is building for.
import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

/// Exactly the checks that must hold on the platform being released.
const REQUIRED_TESTS = [
  'zvec_storage::tests::full_text_search_handles_terms_phrases_case_unicode_updates_and_deletes',
  'zvec_storage::tests::full_text_search_preserves_tenant_scope_and_bounds',
  'zvec_storage::tests::migrates_a_vector_only_collection_without_losing_records',
  'zvec_storage::tests::migration_rolls_back_an_unpublished_staging_directory_after_restart',
  'zvec_storage::tests::migration_publishes_a_verified_ready_stage_after_restart',
  'zvec_storage::tests::rebuilds_a_missing_derived_collection_from_authoritative_records',
  'zvec_storage::tests::rejects_a_content_field_with_the_wrong_index_type',
  'zvec_storage::tests::abnormal_shutdown_recovers_committed_write',
];

const command = 'cargo';
const commonArguments = [
  'test',
  '--locked',
  '-p',
  'fm-semantic-worker',
  '--lib',
  '--features',
  'zvec',
];
const plan = {
  command,
  commonArguments,
  tests: REQUIRED_TESTS,
  invocations: REQUIRED_TESTS.map((name) => [...commonArguments, '--', '--exact', name]),
};

const reportArgument = process.argv.indexOf('--report');
const reportPath =
  reportArgument >= 0
    ? process.argv[reportArgument + 1]
    : fileURLToPath(new URL('../docs/evaluations/knowledge-retrieval-v1.json', import.meta.url));
const reportValidatorArguments =
  reportPath === undefined
    ? []
    : [
        'run',
        '--quiet',
        '--locked',
        '-p',
        'fm-application',
        '--example',
        'validate_knowledge_release_report',
        '--',
        reportPath,
      ];
plan.reportValidator = {
  command: 'cargo',
  arguments: reportValidatorArguments,
};

if (process.argv.includes('--print-plan')) {
  console.log(JSON.stringify(plan, null, 2));
  process.exit(0);
}

if (reportPath === undefined) {
  throw new Error('--report requires a path');
}
const report = JSON.parse(readFileSync(reportPath, 'utf8'));
if (
  report.decision !== 'go' ||
  report.productionMeasurement !== true ||
  !Array.isArray(report.blockingReasons) ||
  report.blockingReasons.length !== 0
) {
  throw new Error(
    'the repository knowledge-search evaluation is not a measured go; refusing a qualified release',
  );
}

const reportValidation = spawnSync(plan.reportValidator.command, plan.reportValidator.arguments, {
  encoding: 'utf8',
});
if (reportValidation.error) throw reportValidation.error;
if (reportValidation.status !== 0) {
  process.stderr.write(`${reportValidation.stdout ?? ''}${reportValidation.stderr ?? ''}`);
  throw new Error('the repository knowledge-search evaluation evidence does not support a go');
}
if (process.argv.includes('--check-report')) {
  console.log('Verified a measured knowledge-search go decision.');
  process.exit(0);
}

for (let index = 0; index < REQUIRED_TESTS.length; index += 1) {
  const name = REQUIRED_TESTS[index];
  const result = spawnSync(plan.command, plan.invocations[index], { encoding: 'utf8' });
  if (result.error) throw result.error;
  const output = `${result.stdout ?? ''}${result.stderr ?? ''}`;
  process.stdout.write(output);
  if (result.status !== 0) {
    throw new Error(`${name} failed with status ${result.status ?? 'unknown'}`);
  }

  // A renamed or removed test must fail the release rather than silently pass
  // as an empty filtered run.
  if (!new RegExp(`test ${name} \\.\\.\\. ok`).test(output)) {
    throw new Error(`required full-text check did not run: ${name}`);
  }
}
console.log(`Verified ${REQUIRED_TESTS.length} full-text migration and lifecycle checks.`);
