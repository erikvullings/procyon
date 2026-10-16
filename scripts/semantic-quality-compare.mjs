// Scores user-recorded semantic search results for two or more models (task 0247, item 5).
//
// Input is a local JSON file the user fills in: their own queries, the files known to answer each
// query, and the ranked file paths each model returned. Only paths are compared; the script never
// opens the listed files, reads document content, or contacts the network.
import fs from 'node:fs';
import { fileURLToPath } from 'node:url';

export const CUTOFFS = [1, 3, 5, 10];

export function template() {
  return {
    description: 'Fill in one entry per query. Paths are relative to the enrolled root.',
    models: ['gemma', 'e5'],
    queries: [
      {
        id: 'q01',
        query: 'Write your own natural-language question here',
        expected: ['folder/known-answer.pdf'],
        results: { gemma: ['folder/first-result.pdf', 'folder/second-result.docx'], e5: [] },
      },
    ],
  };
}

export function normalizePath(value) {
  return value.trim().replaceAll('\\', '/').replace(/^\.\//, '').replace(/\/+/g, '/');
}

/** Ranked distinct files: several excerpts from one file count as that file's first rank. */
export function distinctRanked(paths) {
  const seen = new Set();
  const ranked = [];
  for (const item of paths) {
    const normalized = normalizePath(item);
    if (normalized && !seen.has(normalized)) {
      seen.add(normalized);
      ranked.push(normalized);
    }
  }
  return ranked;
}

export function validate(input) {
  if (!Array.isArray(input?.models) || input.models.length < 2) {
    throw new Error('"models" must list at least two model labels');
  }
  if (!Array.isArray(input.queries) || input.queries.length === 0) {
    throw new Error('"queries" must be a non-empty array');
  }
  const ids = new Set();
  for (const query of input.queries) {
    if (typeof query.id !== 'string' || ids.has(query.id)) {
      throw new Error(`every query needs a unique string "id" (got ${JSON.stringify(query.id)})`);
    }
    ids.add(query.id);
    if (!Array.isArray(query.expected) || query.expected.length === 0) {
      throw new Error(`query ${query.id} needs at least one expected path`);
    }
    for (const model of input.models) {
      if (!Array.isArray(query.results?.[model])) {
        throw new Error(`query ${query.id} has no results array for model "${model}"`);
      }
    }
  }
}

/** 1-based rank of the first expected file, or null when absent. */
export function firstRelevantRank(ranked, expected) {
  const relevant = new Set(expected.map(normalizePath));
  const index = ranked.findIndex((item) => relevant.has(item));
  return index < 0 ? null : index + 1;
}

export function scoreQuery(query, model, k) {
  const ranked = distinctRanked(query.results[model]).slice(0, k);
  const expected = new Set(query.expected.map(normalizePath));
  const rank = firstRelevantRank(ranked, [...expected]);
  const found = ranked.filter((item) => expected.has(item)).length;
  return {
    rank,
    reciprocalRank: rank === null ? 0 : 1 / rank,
    recall: found / expected.size,
  };
}

export function compare(input, k = 10) {
  validate(input);
  const perQuery = input.queries.map((query) => ({
    id: query.id,
    scores: Object.fromEntries(input.models.map((model) => [model, scoreQuery(query, model, k)])),
  }));
  const models = {};
  for (const model of input.models) {
    const scores = perQuery.map((entry) => entry.scores[model]);
    const hitAt = {};
    for (const cutoff of CUTOFFS.filter((value) => value <= k)) {
      hitAt[cutoff] = mean(scores.map(({ rank }) => (rank !== null && rank <= cutoff ? 1 : 0)));
    }
    models[model] = {
      hitAt,
      mrr: mean(scores.map(({ reciprocalRank }) => reciprocalRank)),
      recallAtK: mean(scores.map(({ recall }) => recall)),
      misses: scores.filter(({ rank }) => rank === null).length,
    };
  }
  const [first, second] = input.models;
  const headToHead = { [first]: 0, [second]: 0, ties: 0 };
  for (const entry of perQuery) {
    const difference = entry.scores[first].reciprocalRank - entry.scores[second].reciprocalRank;
    if (difference > 0) headToHead[first] += 1;
    else if (difference < 0) headToHead[second] += 1;
    else headToHead.ties += 1;
  }
  return { k, queryCount: perQuery.length, models, headToHead, perQuery };
}

function mean(values) {
  return values.length === 0 ? 0 : values.reduce((sum, value) => sum + value, 0) / values.length;
}

export function formatReport(report) {
  const models = Object.keys(report.models);
  const cutoffs = Object.keys(report.models[models[0]].hitAt);
  const header = [
    'model',
    ...cutoffs.map((cutoff) => `hit@${cutoff}`),
    'MRR',
    `recall@${report.k}`,
    'misses',
  ];
  const rows = models.map((model) => {
    const score = report.models[model];
    return [
      model,
      ...cutoffs.map((cutoff) => score.hitAt[cutoff].toFixed(2)),
      score.mrr.toFixed(3),
      score.recallAtK.toFixed(2),
      String(score.misses),
    ];
  });
  const table = [header, header.map(() => '---'), ...rows].map((row) => `| ${row.join(' | ')} |`);
  const versus = Object.entries(report.headToHead)
    .map(([label, count]) => `${label} ${count}`)
    .join(', ');
  const ranks = report.perQuery.map(
    (entry) =>
      `| ${entry.id} | ${models.map((model) => entry.scores[model].rank ?? '-').join(' | ')} |`,
  );
  return [
    `${report.queryCount} queries, top ${report.k} distinct files per model.`,
    '',
    ...table,
    '',
    `Better first-relevant rank per query: ${versus}.`,
    '',
    `| query | ${models.map((model) => `${model} rank`).join(' | ')} |`,
    `| --- | ${models.map(() => '---').join(' | ')} |`,
    ...ranks,
    '',
  ].join('\n');
}

function main(argv) {
  if (argv[0] === '--template') {
    process.stdout.write(`${JSON.stringify(template(), null, 2)}\n`);
    return;
  }
  const [file, ...rest] = argv;
  if (!file) {
    throw new Error(
      'Usage: node scripts/semantic-quality-compare.mjs <results.json> [--k 10] [--json]\n       node scripts/semantic-quality-compare.mjs --template > results.json',
    );
  }
  const kIndex = rest.indexOf('--k');
  const k = kIndex >= 0 ? Number(rest[kIndex + 1]) : 10;
  if (!Number.isInteger(k) || k <= 0) throw new Error('--k must be a positive integer');
  const report = compare(JSON.parse(fs.readFileSync(file, 'utf8')), k);
  process.stdout.write(
    rest.includes('--json') ? `${JSON.stringify(report, null, 2)}\n` : formatReport(report),
  );
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  try {
    main(process.argv.slice(2).filter((argument) => argument !== '--'));
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
