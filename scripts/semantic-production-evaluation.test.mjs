import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { test } from 'node:test';

import { semanticEvaluationAggregationPlan } from './aggregate-semantic-production-evaluation.mjs';

const targets = ['linux-aarch64', 'linux-x86_64', 'macos-aarch64', 'windows-x86_64'];

function fixture() {
  return fs.mkdtempSync(path.join(tmpdir(), 'semantic-production-evaluation-'));
}

function measurement(target) {
  return {
    identity: { target },
    productionPackage: true,
    metrics: {
      retrieval: {
        fileRecallAtK: 1,
        chunkRecallAtK: 1,
        meanReciprocalRank: 1,
        ndcgAtK: 1,
      },
      negativeControlFalsePositiveRate: 0,
      offlineCitationCorrectness: 1,
      offlineCitationRecall: 1,
      groundedAnswerCitationCorrectness: null,
      groundedAnswerCitationRecall: null,
    },
  };
}

function writeReports(root, selectedTargets = targets) {
  const template = JSON.parse(
    fs.readFileSync('docs/evaluations/semantic-production-v1.json', 'utf8'),
  );
  for (const target of selectedTargets) {
    const directory = path.join(root, `semantic-payloads-${target}`);
    fs.mkdirSync(directory, { recursive: true });
    fs.writeFileSync(
      path.join(directory, 'semantic-production-evaluation.json'),
      JSON.stringify({ ...template, measurements: [measurement(target)] }),
    );
  }
}

test('supported-target semantic evidence is handed to the typed Rust aggregator', () => {
  const root = fixture();
  writeReports(root);
  const output = path.join(root, 'aggregate', 'report.json');
  const reviewed = path.resolve('docs/evaluations/semantic-production-v1.json');
  const plan = semanticEvaluationAggregationPlan(root, output, reviewed);
  assert.equal(plan.command, 'cargo');
  assert.match(plan.arguments.join(' '), /aggregate_semantic_production_evaluation/u);
  assert.equal(
    plan.arguments.filter((argument) => /semantic-production-evaluation\.json$/u.test(argument))
      .length,
    4,
  );
  assert.ok(plan.arguments.includes(reviewed));
  assert.doesNotMatch(
    fs.readFileSync('docs/evaluations/semantic-production-v1.json', 'utf8'),
    /"query"|"content"|"excerpt"/u,
  );
});

test('semantic evidence aggregation rejects a missing supported target', () => {
  const root = fixture();
  writeReports(root, targets.slice(0, 3));
  const result = spawnSync(
    'node',
    [
      'scripts/aggregate-semantic-production-evaluation.mjs',
      root,
      path.join(root, 'report.json'),
      'docs/evaluations/semantic-production-v1.json',
    ],
    { encoding: 'utf8' },
  );
  assert.notEqual(result.status, 0);
  assert.match(`${result.stdout}${result.stderr}`, /exactly four supported-target/u);
});
