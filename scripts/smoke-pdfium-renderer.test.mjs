import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import fs from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import test from 'node:test';

import { parseRenderedPdfPage, smokePackagedPdfiumRenderer } from './smoke-pdfium-renderer.mjs';

function report(overrides = {}) {
  const image = Buffer.from([0xff, 0xd8, 0xff, 0xd9]);
  const header = Buffer.from(
    JSON.stringify({
      page_number: 1,
      image_index: 0,
      rendered_page: true,
      media_type: 'image/jpeg',
      width: 32,
      height: 32,
      byte_length: image.length,
      ...overrides,
    }),
  );
  const counts = Buffer.alloc(12);
  counts.writeUInt32BE(1, 0);
  counts.writeUInt32BE(1, 4);
  counts.writeUInt32BE(header.length, 8);
  return Buffer.concat([counts, header, image]);
}

test('accepts only complete bounded, honestly cited page-rendered JPEG evidence', () => {
  assert.deepEqual(parseRenderedPdfPage(report()), { width: 32, height: 32, bytes: 4 });
  for (const fields of [
    { rendered_page: false },
    { page_number: 2 },
    { image_index: 2 },
    { byte_length: 5 },
    { width: 5000, height: 5000 },
  ]) {
    assert.throws(() => parseRenderedPdfPage(report(fields)), /bounded page-rendered/u);
  }
  assert.throws(() => parseRenderedPdfPage(report().subarray(0, 8)), /incomplete/u);
  assert.throws(() => parseRenderedPdfPage(report().subarray(0, -1)), /bounded page-rendered/u);
});

test('packaged PDF renderer rejects unpaired and absent signed artifacts', async () => {
  const directory = fs.mkdtempSync(path.join(tmpdir(), 'procyon-pdfium-catalog-test-'));
  try {
    const empty = { catalog: { artifacts: [] } };
    assert.equal(
      await smokePackagedPdfiumRenderer(directory, empty, directory, {}, directory),
      undefined,
    );
    const loader = {
      id: 'pdfium-library',
      component_id: 'procyon.semantic.pdfium-runtime',
      checksum: Array(32).fill(0),
      resources: { download_bytes: 8 },
    };
    const bundle = { catalog: { artifacts: [loader] } };
    await assert.rejects(
      smokePackagedPdfiumRenderer(directory, bundle, directory, {}, directory),
      /must both be present/u,
    );
    bundle.catalog.artifacts.push({
      ...loader,
      id: 'pdfium-notices',
      component_id: 'procyon.semantic.pdfium-notices',
    });
    await assert.rejects(
      smokePackagedPdfiumRenderer(directory, bundle, directory, {}, directory),
      /differs from catalog input/u,
    );
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
});

test('isolated development renderer produces real PDF page evidence', (context) => {
  const worker = process.env.PROCYON_TEST_PDFIUM_WORKER;
  if (!worker) {
    context.skip('set PROCYON_TEST_PDFIUM_WORKER to a verified isolated development worker');
    return;
  }
  assert.ok(
    fs.realpathSync(worker).startsWith(fs.realpathSync(path.join(process.cwd(), 'target'))),
    'renderer test worker must be isolated under target/',
  );
  const directory = fs.mkdtempSync(path.join(tmpdir(), 'procyon-pdfium-smoke-test-'));
  try {
    const fixture = path.join(directory, 'one-page-rle-scan.pdf');
    const generated = spawnSync(
      process.execPath,
      [
        'scripts/generate-large-pdf-fixture.mjs',
        '--output',
        fixture,
        '--variant',
        'rle-scan',
        '--pages',
        '1',
        '--width',
        '32',
        '--height',
        '32',
      ],
      { timeout: 10_000 },
    );
    assert.equal(generated.status, 0, generated.stderr.toString());
    const source = fs.readFileSync(fixture);
    const digest = createHash('sha256').update(source).digest('hex');
    const rendered = spawnSync(worker, ['--pdf-visual-pages-child', digest], {
      input: source,
      timeout: 15_000,
      maxBuffer: 4 * 1024 * 1024,
    });
    assert.equal(rendered.status, 0, rendered.stderr.toString());
    const page = parseRenderedPdfPage(rendered.stdout);
    assert.ok(page.bytes > 100);
    assert.ok(page.width > 0 && page.height > 0);
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
});
