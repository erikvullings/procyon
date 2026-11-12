// Compare Lattice's CPU and Metal image paths with Procyon's pinned Python fixture.
// Usage: node scripts/verify-lattice-embeddinggemma-vision.mjs <lattice-checkout> <model-dir>

import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(fileURLToPath(new URL('..', import.meta.url)));
const [latticePath, modelPath] = process.argv.slice(2);
if (!latticePath || !modelPath) {
  throw new Error(
    'usage: node scripts/verify-lattice-embeddinggemma-vision.mjs <lattice-checkout> <model-dir>',
  );
}
const lattice = resolve(latticePath);
const model = resolve(modelPath);
const binary = resolve(lattice, 'target/release/examples/dump_embeddinggemma2_vision');
const output = resolve(root, 'target/lattice-1889-results');
await mkdir(output, { recursive: true });

const width = 128;
const height = 96;
const pixels = Buffer.alloc(width * height * 3);
for (let y = 0; y < height; y++) {
  for (let x = 0; x < width; x++) {
    const offset = (y * width + x) * 3;
    pixels[offset] = (x * 2 + y) % 256;
    pixels[offset + 1] = (x + y * 2) % 256;
    pixels[offset + 2] = (x + y) % 256;
  }
}
const rgb = resolve(output, 'procyon-pattern-128x96.rgb');
const corpus = resolve(output, 'corpus.json');
await writeFile(rgb, pixels);
await writeFile(
  corpus,
  JSON.stringify([
    { id: 'procyon-pattern', parts: [{ image: { raw_rgb8_path: rgb, width, height } }] },
  ]),
);
console.log(`fixture SHA-256: ${createHash('sha256').update(pixels).digest('hex')}`);
console.log(`Lattice: ${lattice}; model: ${model}`);

function run(backend) {
  const destination = resolve(output, `${backend}.safetensors`);
  const args = [
    binary,
    '--model-dir',
    model,
    '--corpus',
    corpus,
    '--out',
    destination,
    '--widths',
    '768,512,256,128',
  ];
  if (backend === 'metal') args.push('--metal');
  const started = performance.now();
  const result = spawnSync('/usr/bin/time', ['-l', ...args], { encoding: 'utf8' });
  const seconds = (performance.now() - started) / 1000;
  if (result.status !== 0) {
    throw new Error(`${backend} failed (${result.status}):\n${result.stderr || result.error}`);
  }
  const rss = result.stderr.match(/(\d+)\s+maximum resident set size/);
  console.log(
    `${backend}: ${seconds.toFixed(3)} s, peak RSS ${rss ? Number(rss[1]) : 'unknown'} bytes`,
  );
  console.log(
    result.stderr
      .split('\n')
      .filter((line) => line.includes('@@mvt'))
      .join('\n'),
  );
  return destination;
}

async function readDump(path) {
  const data = await readFile(path);
  const length = Number(data.readBigUInt64LE(0));
  const header = JSON.parse(data.subarray(8, 8 + length).toString());
  const start = 8 + length;
  const tensor = (name) => {
    const entry = header[`procyon-pattern/${name}`];
    if (!entry) throw new Error(`missing tensor ${name}`);
    const [from, to] = entry.data_offsets;
    const values = [];
    for (let offset = start + from; offset < start + to; offset += 4) {
      values.push(data.readFloatLE(offset));
    }
    return values;
  };
  return { tensor };
}

const reference = JSON.parse(
  await readFile(
    resolve(root, 'crates/fm-semantic-worker/tests/embeddinggemma-image-reference-v1.json'),
    'utf8',
  ),
);
if (reference.revision !== '914f7f89142e33e77833254d9c9b90c3cef7303b') {
  throw new Error(`unexpected reference revision: ${reference.revision}`);
}
const cpu = await readDump(run('cpu'));
const metal = await readDump(run('metal'));
const ids = cpu.tensor('layout.input_ids');
if (JSON.stringify(ids) !== JSON.stringify(reference.ids)) {
  console.error(`input IDs differ: Lattice ${ids.length}, reference ${reference.ids.length}`);
  process.exitCode = 1;
}
if (JSON.stringify(ids) !== JSON.stringify(metal.tensor('layout.input_ids'))) {
  throw new Error('CPU/Metal input IDs differ');
}
console.log(`input IDs: ${ids.length}, matching reference: ${!process.exitCode}`);
for (const [name, expected] of [
  ['layout.valid_patch_counts', 2394],
  ['layout.soft_token_counts', 266],
]) {
  const cpuCount = cpu.tensor(name);
  const metalCount = metal.tensor(name);
  console.log(`${name}: CPU ${cpuCount}, Metal ${metalCount}`);
  if (
    cpuCount.length !== 1 ||
    cpuCount[0] !== expected ||
    metalCount.length !== 1 ||
    metalCount[0] !== expected
  ) {
    process.exitCode = 1;
  }
}
console.log(
  'width  CPU/reference cosine  Metal/reference cosine  CPU/Metal cosine  CPU/ref max abs  Metal/ref max abs',
);
for (const dimension of [128, 256, 512, 768]) {
  const expected = reference.vector.slice(0, dimension);
  const norm = Math.hypot(...expected);
  const normalized = expected.map((value) => value / norm);
  const a = cpu.tensor(`readout.normalized_${dimension}`);
  const b = metal.tensor(`readout.normalized_${dimension}`);
  if (a.length !== dimension || b.length !== dimension)
    throw new Error(`wrong dimension ${dimension}`);
  const metrics = (left, right) => ({
    cosine:
      left.reduce((sum, value, index) => sum + value * right[index], 0) /
      (Math.hypot(...left) * Math.hypot(...right)),
    maxAbs: Math.max(...left.map((value, index) => Math.abs(value - right[index]))),
  });
  const cpuRef = metrics(a, normalized);
  const metalRef = metrics(b, normalized);
  const cpuMetal = metrics(a, b);
  console.log(
    `${dimension}    ${cpuRef.cosine.toFixed(9)}            ${metalRef.cosine.toFixed(9)}              ${cpuMetal.cosine.toFixed(9)}          ${cpuRef.maxAbs.toExponential(3)}       ${metalRef.maxAbs.toExponential(3)}`,
  );
  if (
    cpuRef.cosine < 0.99999 ||
    metalRef.cosine < 0.99999 ||
    cpuMetal.cosine < 0.99999 ||
    cpuRef.maxAbs > 5e-5 ||
    metalRef.maxAbs > 5e-5
  ) {
    process.exitCode = 1;
  }
}
