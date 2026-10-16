import assert from 'node:assert/strict';
import test from 'node:test';
import {
  compare,
  distinctRanked,
  formatReport,
  template,
  validate,
} from './semantic-quality-compare.mjs';

const input = {
  models: ['gemma', 'e5'],
  queries: [
    {
      id: 'q1',
      query: 'q',
      expected: ['docs/a.pdf'],
      results: { gemma: ['docs\\a.pdf', 'b.txt'], e5: ['b.txt', 'c.txt', './docs/a.pdf'] },
    },
    {
      id: 'q2',
      query: 'q',
      expected: ['x.md', 'y.md'],
      results: { gemma: ['z.md', 'z.md', 'y.md'], e5: ['x.md'] },
    },
    { id: 'q3', query: 'q', expected: ['m.md'], results: { gemma: [], e5: [] } },
  ],
};

test('duplicate excerpts collapse to one ranked file and separators are normalized', () => {
  assert.deepEqual(distinctRanked(['a\\b.pdf', './a/b.pdf', 'c.pdf', 'a//b.pdf']), [
    'a/b.pdf',
    'c.pdf',
  ]);
});

test('metrics reflect first-relevant ranks per model', () => {
  const report = compare(input, 10);
  assert.equal(report.queryCount, 3);
  assert.deepEqual(
    report.perQuery.map(({ scores }) => [scores.gemma.rank, scores.e5.rank]),
    [
      [1, 3],
      [2, 1],
      [null, null],
    ],
  );
  assert.ok(Math.abs(report.models.gemma.mrr - 0.5) < 1e-9);
  assert.ok(Math.abs(report.models.e5.mrr - (1 / 3 + 1) / 3) < 1e-9);
  assert.ok(Math.abs(report.models.gemma.hitAt[1] - 1 / 3) < 1e-9);
  assert.ok(Math.abs(report.models.e5.hitAt[3] - 2 / 3) < 1e-9);
  assert.equal(report.models.gemma.misses, 1);
  assert.deepEqual(report.headToHead, { gemma: 1, e5: 1, ties: 1 });
});

test('a smaller k excludes later hits and cutoffs above k', () => {
  const report = compare(input, 2);
  assert.equal(report.perQuery[0].scores.e5.rank, null);
  assert.deepEqual(Object.keys(report.models.gemma.hitAt), ['1']);
});

test('invalid input is rejected with a useful message', () => {
  assert.throws(() => validate({ models: ['gemma'], queries: [] }), /two model labels/);
  assert.throws(
    () =>
      validate({
        models: ['gemma', 'e5'],
        queries: [{ id: 'q', expected: ['a'], results: { gemma: [] } }],
      }),
    /e5/,
  );
  assert.throws(
    () =>
      validate({
        models: ['a', 'b'],
        queries: [
          { id: 'q', expected: ['x'], results: { a: [], b: [] } },
          { id: 'q', expected: ['x'], results: { a: [], b: [] } },
        ],
      }),
    /unique/,
  );
});

test('the template is valid input and the report renders every model', () => {
  const report = compare(template(), 10);
  const text = formatReport(report);
  assert.match(text, /gemma/);
  assert.match(text, /e5/);
});
