import assert from 'node:assert/strict';
import { test } from 'node:test';

import { newestRunSince, nextSemanticTag } from './resign-gemma-candidate.mjs';

test('increments the trailing component number', () => {
  assert.equal(nextSemanticTag('semantic-v0.7.5'), 'semantic-v0.7.6');
  assert.equal(nextSemanticTag('semantic-v9'), 'semantic-v10');
  assert.equal(nextSemanticTag('semantic-v0.7.9'), 'semantic-v0.7.10');
});

test('appends .1 to a tag without a trailing number', () => {
  assert.equal(nextSemanticTag('semantic-beta'), 'semantic-beta.1');
});

test('picks the newest run created after the dispatch and ignores older ones', () => {
  const since = Date.parse('2026-11-03T16:00:00Z');
  const runs = [
    { databaseId: 1, createdAt: '2026-11-03T15:00:00Z' },
    { databaseId: 2, createdAt: '2026-11-03T16:00:05Z' },
    { databaseId: 3, createdAt: '2026-11-03T16:00:30Z' },
  ];
  assert.equal(newestRunSince(runs, since)?.databaseId, 3);
  assert.equal(newestRunSince(runs.slice(0, 1), since), undefined);
});
