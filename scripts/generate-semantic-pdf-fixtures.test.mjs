import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { inflateSync } from 'node:zlib';

import { generateSemanticPdfFixture } from './generate-semantic-pdf-fixtures.mjs';

function fixture(context, name, textLayer) {
  const root = mkdtempSync(join(tmpdir(), 'procyon-pdf-fixture-'));
  context.after(() => rmSync(root, { recursive: true, force: true }));
  const filename = join(root, name);
  const result = generateSemanticPdfFixture(filename, {
    pages: 3,
    width: 16,
    height: 12,
    textLayer,
  });
  return { filename, result, bytes: readFileSync(filename) };
}

function inspectPdf(bytes, textLayer) {
  const ascii = bytes.toString('latin1');
  assert.ok(ascii.startsWith('%PDF-1.4\n'));
  assert.match(ascii, /\/Count 3\b/u);
  const xrefOffset = Number(ascii.match(/startxref\n(\d+)\n%%EOF\n$/u)?.[1]);
  assert.ok(Number.isSafeInteger(xrefOffset));
  const xref = ascii.slice(xrefOffset).split('\n');
  assert.equal(xref[0], 'xref');
  assert.equal(xref[1], '0 13');
  for (let id = 1; id <= 12; id++) {
    const offset = Number(xref[id + 2].slice(0, 10));
    assert.ok(ascii.startsWith(`${id} 0 obj\n`, offset), `object ${id} has a valid xref`);
  }
  for (let page = 1; page <= 3; page++) {
    const contentId = 5 + 3 * (page - 1);
    const contentStart = Number(xref[contentId + 2].slice(0, 10));
    const contentEnd = ascii.indexOf('endobj', contentStart);
    const content = ascii.slice(contentStart, contentEnd);
    assert.equal(content.includes(`Synthetic page ${page} of 3`), textLayer);
    const imageId = contentId + 1;
    const imageStart = Number(xref[imageId + 2].slice(0, 10));
    const imageHeader = ascii.slice(imageStart, imageStart + 200);
    const length = Number(imageHeader.match(/\/Length (\d+)/u)?.[1]);
    const streamStart = bytes.indexOf(Buffer.from('stream\n'), imageStart) + 7;
    assert.equal(inflateSync(bytes.subarray(streamStart, streamStart + length)).length, 576);
  }
}

test('small scanned and text fixtures are reproducible, page-local PDFs', (context) => {
  for (const textLayer of [false, true]) {
    const { filename, result, bytes } = fixture(
      context,
      textLayer ? 'text.pdf' : 'scan.pdf',
      textLayer,
    );
    assert.equal(result.pages, 3);
    assert.equal(result.bytes, bytes.length);
    inspectPdf(bytes, textLayer);
    const repeat = join(dirname(filename), `repeat-${textLayer}.pdf`);
    generateSemanticPdfFixture(repeat, { pages: 3, width: 16, height: 12, textLayer });
    assert.equal(
      createHash('sha256').update(bytes).digest('hex'),
      createHash('sha256').update(readFileSync(repeat)).digest('hex'),
    );
    assert.throws(
      () => generateSemanticPdfFixture(filename, { pages: 3, width: 16, height: 12, textLayer }),
      /already exists/u,
    );
    assert.deepEqual(readFileSync(filename), bytes);
  }
});

test('fixture generation rejects unbounded pages before writing', (context) => {
  const { filename } = fixture(context, 'valid.pdf', false);
  assert.throws(
    () =>
      generateSemanticPdfFixture(join(dirname(filename), 'oversized.pdf'), {
        pages: 1,
        width: 4_000,
        height: 4_000,
        textLayer: false,
      }),
    /positive and bounded/u,
  );
});
