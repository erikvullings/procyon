import assert from 'node:assert/strict';
import { test } from 'node:test';
import { download } from './fetch-approved-semantic-catalog.mjs';

const URL = 'https://example.invalid/catalog.json';

function scriptedFetch(outcomes) {
  const calls = [];
  const fetchImpl = async (url) => {
    calls.push(url);
    const outcome = outcomes.shift();
    if (outcome instanceof Error) {
      throw outcome;
    }
    return new Response(outcome.body ?? null, { status: outcome.status });
  };
  return { calls, fetchImpl };
}

test('download retries transient HTTP and network failures with backoff', async () => {
  const { calls, fetchImpl } = scriptedFetch([
    { status: 500 },
    new TypeError('fetch failed'),
    { status: 429 },
    { status: 200, body: 'catalog' },
  ]);
  const delays = [];
  const bytes = await download(URL, { fetchImpl, sleep: async (ms) => delays.push(ms) });
  assert.equal(bytes.toString(), 'catalog');
  assert.equal(calls.length, 4);
  assert.deepEqual(delays, [2_000, 4_000, 8_000]);
});

test('download fails immediately on a non-transient HTTP status', async () => {
  const { calls, fetchImpl } = scriptedFetch([{ status: 404 }]);
  await assert.rejects(download(URL, { fetchImpl, sleep: async () => {} }), /HTTP 404/);
  assert.equal(calls.length, 1);
});

test('download gives up after the final transient failure', async () => {
  const { calls, fetchImpl } = scriptedFetch(Array.from({ length: 4 }, () => ({ status: 503 })));
  await assert.rejects(download(URL, { fetchImpl, sleep: async () => {} }), /HTTP 503/);
  assert.equal(calls.length, 4);
});
