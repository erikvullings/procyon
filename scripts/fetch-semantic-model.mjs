// Downloads the pinned multilingual embedding model used by the semantic
// developer bundle into a content-verified local cache.
//
// Every file is pinned to one immutable Hugging Face revision and verified by
// exact byte length and SHA-256 before it is accepted. Nothing here runs inside
// the semantic worker: the worker never performs network access, it only reads
// the packed artifact this cache feeds into the signed developer catalog.

import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const repositoryRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

/** Immutable upstream identity of the developer-bundle multilingual model. */
export const MULTILINGUAL_MODEL = {
  repository: 'intfloat/multilingual-e5-small',
  revision: '614241f622f53c4eeff9890bdc4f31cfecc418b3',
  license: 'MIT',
  dimensions: 384,
  maxInputTokens: 512,
  files: [
    {
      name: 'model.onnx',
      remote: 'onnx/model.onnx',
      bytes: 470268510,
      sha256: 'ca456c06b3a9505ddfd9131408916dd79290368331e7d76bb621f1cba6bc8665',
    },
    {
      name: 'tokenizer.json',
      remote: 'tokenizer.json',
      bytes: 17082730,
      sha256: '0b44a9d7b51c3c62626640cda0e2c2f70fdacdc25bbbd68038369d14ebdf4c39',
    },
    {
      name: 'config.json',
      remote: 'config.json',
      bytes: 655,
      sha256: '69137736cab8b8903a07fe8afaafdda25aac55415a12a55d1bffa9f581abf959',
    },
    {
      name: 'tokenizer_config.json',
      remote: 'tokenizer_config.json',
      bytes: 443,
      sha256: 'a1d6bc8734a6f635dc158508bef000f8e2e5a759c7d92f984b2c86e5ff53425b',
    },
    {
      name: 'special_tokens_map.json',
      remote: 'special_tokens_map.json',
      bytes: 167,
      sha256: 'd05497f1da52c5e09554c0cd874037a083e1dc1b9cfd48034d1c717f1afc07a7',
    },
  ],
};

function digestOf(file) {
  const hash = createHash('sha256');
  const handle = fs.openSync(file, 'r');
  try {
    const buffer = Buffer.allocUnsafe(4 * 1024 * 1024);
    for (;;) {
      const read = fs.readSync(handle, buffer, 0, buffer.length, null);
      if (read === 0) break;
      hash.update(buffer.subarray(0, read));
    }
  } finally {
    fs.closeSync(handle);
  }
  return hash.digest('hex');
}

function isAlreadyCached(destination, descriptor) {
  if (!fs.existsSync(destination)) return false;
  if (fs.statSync(destination).size !== descriptor.bytes) return false;
  return digestOf(destination) === descriptor.sha256;
}

async function download(url, destination, descriptor) {
  const partial = `${destination}.partial`;
  fs.rmSync(partial, { force: true });
  const response = await fetch(url, { redirect: 'follow' });
  if (!response.ok || !response.body) {
    throw new Error(`${url} returned HTTP ${response.status}`);
  }
  const handle = await fs.promises.open(partial, 'w');
  let written = 0;
  let reported = 0;
  try {
    for await (const chunk of response.body) {
      await handle.write(chunk);
      written += chunk.length;
      if (written - reported >= 32 * 1024 * 1024 || written === descriptor.bytes) {
        reported = written;
        const percent = ((written / descriptor.bytes) * 100).toFixed(1);
        process.stderr.write(
          `  ${descriptor.name}: ${(written / 1024 / 1024).toFixed(0)} MiB (${percent}%)\n`,
        );
      }
    }
  } finally {
    await handle.close();
  }
  if (written !== descriptor.bytes) {
    fs.rmSync(partial, { force: true });
    throw new Error(
      `${descriptor.name} downloaded ${written} bytes but the pinned revision declares ${descriptor.bytes}`,
    );
  }
  const digest = digestOf(partial);
  if (digest !== descriptor.sha256) {
    fs.rmSync(partial, { force: true });
    throw new Error(
      `${descriptor.name} hashed ${digest} but the pinned revision declares ${descriptor.sha256}`,
    );
  }
  fs.renameSync(partial, destination);
}

function defaultCacheRoot() {
  const metadata = JSON.parse(
    execFileSync('cargo', ['metadata', '--format-version=1', '--no-deps'], {
      cwd: repositoryRoot,
      env: process.env,
      encoding: 'utf8',
    }),
  );
  return path.join(metadata.target_directory, 'semantic-model-cache');
}

/**
 * Ensures every pinned file is present and verified, then returns the cache
 * directory holding exactly those files.
 */
export async function fetchMultilingualModel(cacheRoot = defaultCacheRoot()) {
  const directory = path.join(
    cacheRoot,
    MULTILINGUAL_MODEL.repository.replace('/', '--'),
    MULTILINGUAL_MODEL.revision,
  );
  fs.mkdirSync(directory, { recursive: true });
  const total = MULTILINGUAL_MODEL.files.reduce((sum, file) => sum + file.bytes, 0);
  let cached = 0;
  for (const descriptor of MULTILINGUAL_MODEL.files) {
    const destination = path.join(directory, descriptor.name);
    if (isAlreadyCached(destination, descriptor)) {
      cached += descriptor.bytes;
      continue;
    }
    const url = `https://huggingface.co/${MULTILINGUAL_MODEL.repository}/resolve/${MULTILINGUAL_MODEL.revision}/${descriptor.remote}`;
    process.stderr.write(
      `Downloading ${descriptor.name} (${(descriptor.bytes / 1024 / 1024).toFixed(0)} MiB) from the pinned revision ${MULTILINGUAL_MODEL.revision}\n`,
    );
    // eslint-disable-next-line no-await-in-loop -- sequential downloads keep progress readable and bandwidth bounded.
    await download(url, destination, descriptor);
    cached += descriptor.bytes;
  }
  process.stderr.write(
    `Verified ${(cached / 1024 / 1024).toFixed(0)} MiB of ${(total / 1024 / 1024).toFixed(0)} MiB in ${directory}\n`,
  );
  return directory;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const directory = await fetchMultilingualModel();
  console.log(directory);
}
