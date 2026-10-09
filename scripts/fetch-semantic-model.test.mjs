import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';

import { fetchPinnedModel } from './fetch-semantic-model.mjs';

test('a verified pinned model cache requires no network access', async (t) => {
  const cacheRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'procyon-model-cache-'));
  t.after(() => fs.rmSync(cacheRoot, { recursive: true, force: true }));
  const directory = path.join(cacheRoot, 'example--model', 'revision');
  fs.mkdirSync(directory, { recursive: true });
  const contents = 'verified fixture';
  fs.writeFileSync(path.join(directory, 'tokenizer.json'), contents);
  const originalFetch = globalThis.fetch;
  t.after(() => {
    globalThis.fetch = originalFetch;
  });
  globalThis.fetch = () => {
    throw new Error('unexpected network access');
  };

  const result = await fetchPinnedModel(
    {
      repository: 'example/model',
      revision: 'revision',
      files: [
        {
          name: 'tokenizer.json',
          remote: 'tokenizer.json',
          bytes: Buffer.byteLength(contents),
          sha256: createHash('sha256').update(contents).digest('hex'),
        },
      ],
    },
    cacheRoot,
  );
  assert.equal(result, directory);
});
