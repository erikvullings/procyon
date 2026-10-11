import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { after, test } from 'node:test';

const directory = mkdtempSync(join(tmpdir(), 'procyon-large-pdf-fixture-'));
const script = new URL('./generate-large-pdf-fixture.mjs', import.meta.url).pathname;
after(() => rmSync(directory, { recursive: true, force: true }));

function generate(name, variant, pages = 2, width = 32, height = 24) {
  const output = join(directory, name);
  execFileSync(process.execPath, [
    script,
    '--output',
    output,
    '--variant',
    variant,
    '--pages',
    String(pages),
    '--width',
    String(width),
    '--height',
    String(height),
  ]);
  return readFileSync(output);
}

function assertPdfXref(pdf) {
  const trailer = pdf.toString('latin1').match(/startxref\n(\d+)\n%%EOF\n$/);
  assert.ok(trailer, 'PDF trailer exists');
  const table = pdf.subarray(Number(trailer[1])).toString('latin1');
  const lines = table.match(/^xref\n0 (\d+)\n0000000000 65535 f \n((?:\d{10} 00000 n \n)+)/);
  assert.ok(lines, 'xref entries exist');
  const offsets = [...lines[2].matchAll(/(\d{10}) 00000 n/g)].map((match) => Number(match[1]));
  assert.equal(offsets.length + 1, Number(lines[1]));
  offsets.forEach((offset, index) => {
    assert.ok(
      pdf
        .subarray(offset)
        .toString('latin1')
        .startsWith(`${index + 1} 0 obj\n`),
    );
  });
}

test('generates deterministic, validly framed image-only pages without private inputs', () => {
  const first = generate('first.pdf', 'scanned');
  const second = generate('second.pdf', 'scanned');
  assertPdfXref(first);
  assert.deepEqual(
    {
      header: first.subarray(0, 8).toString(),
      pages: first.toString('latin1').match(/\/Count 2\b/)?.[0],
      images: [...first.toString('latin1').matchAll(/\/Subtype \/Image\b/g)].length,
      text: first.includes('Synthetic search marker'),
      digest: createHash('sha256').update(first).digest('hex'),
    },
    {
      header: '%PDF-1.7',
      pages: '/Count 2',
      images: 2,
      text: false,
      digest: createHash('sha256').update(second).digest('hex'),
    },
  );
});

test('makes a five-page text-layer PDF with a known searchable page marker', () => {
  const pdf = generate('text.pdf', 'text', 5);
  assertPdfXref(pdf);
  assert.match(pdf.toString('latin1'), /Synthetic search marker on page 5/);
  assert.equal([...pdf.toString('latin1').matchAll(/\/Subtype \/Image\b/g)].length, 5);
});

test('refuses to overwrite or generate beyond its fixed source bound', () => {
  generate('existing.pdf', 'scanned');
  const existing = spawnSync(process.execPath, [
    script,
    '--output',
    join(directory, 'existing.pdf'),
    '--variant',
    'scanned',
    '--pages',
    '2',
    '--width',
    '32',
    '--height',
    '24',
  ]);
  assert.notEqual(existing.status, 0);
  const tooLarge = spawnSync(process.execPath, [
    script,
    '--output',
    join(directory, 'too-large.pdf'),
    '--variant',
    'text',
    '--pages',
    '4096',
    '--width',
    '1024',
    '--height',
    '1024',
  ]);
  assert.notEqual(tooLarge.status, 0);
});
