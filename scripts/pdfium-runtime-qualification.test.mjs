import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import test from 'node:test';

import {
  pdfiumTarget,
  preparePinnedPdfium,
  verifyPdfiumArchive,
  verifyPdfiumMembers,
  writePdfiumNotices,
} from './pdfium-runtime-qualification.mjs';
import { sha256Of } from './zvec-runtime-qualification.mjs';

test('pins exactly the four experimental desktop and worker targets', () => {
  for (const [os, arch] of [
    ['darwin', 'arm64'],
    ['linux', 'arm64'],
    ['linux', 'x64'],
    ['win32', 'x64'],
  ]) {
    const descriptor = pdfiumTarget(os, arch);
    assert.match(descriptor.archive.sha256, /^[a-f0-9]{64}$/u);
    assert.match(descriptor.loader.sha256, /^[a-f0-9]{64}$/u);
    assert.ok(descriptor.loader.bytes > 0);
  }
  assert.throws(() => pdfiumTarget('darwin', 'x64'), /No pinned PDFium/u);
  assert.throws(() => pdfiumTarget('win32', 'arm64'), /No pinned PDFium/u);
});

test('rejects missing, duplicate and unsafe archive member lists', () => {
  const descriptor = pdfiumTarget('darwin', 'arm64');
  assert.throws(() => verifyPdfiumMembers(['LICENSE'], descriptor), /missing VERSION/u);
  assert.throws(
    () => verifyPdfiumMembers(['LICENSE', 'LICENSE'], descriptor),
    /unsafe or duplicate/u,
  );
  assert.throws(() => verifyPdfiumMembers(['../LICENSE'], descriptor), /unsafe or duplicate/u);
});

test('rejects a changed archive before extraction', () => {
  const directory = fs.mkdtempSync(path.join(process.cwd(), 'target', 'pdfium-test-'));
  try {
    const file = path.join(directory, 'pdfium-mac-arm64.tgz');
    fs.writeFileSync(file, 'not a pinned PDFium archive');
    assert.throws(
      () => verifyPdfiumArchive(file, pdfiumTarget('darwin', 'arm64')),
      /pinned PDFium archive has/u,
    );
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
});

test('verifies pinned local four-target archives and preserves target-specific notices', async (context) => {
  const directory = process.env.PROCYON_TEST_PDFIUM_ARCHIVES;
  if (!directory) {
    context.skip('set PROCYON_TEST_PDFIUM_ARCHIVES to the four locally retained pinned archives');
    return;
  }
  for (const [os, arch] of [
    ['darwin', 'arm64'],
    ['linux', 'arm64'],
    ['linux', 'x64'],
    ['win32', 'x64'],
  ]) {
    const descriptor = pdfiumTarget(os, arch);
    const archive = path.join(directory, descriptor.archive.name);
    const content = verifyPdfiumArchive(archive, descriptor);
    assert.equal(content.notices.size, 18);
    assert.throws(
      () =>
        verifyPdfiumArchive(archive, {
          ...descriptor,
          loader: { ...descriptor.loader, machine: 0 },
        }),
      /architecture differs/u,
    );
    const prepared = await preparePinnedPdfium(descriptor, directory);
    assert.equal(sha256Of(prepared.library), descriptor.loader.sha256);
    assert.equal(fs.readdirSync(prepared.notices).length, 17);
    assert.ok(fs.statSync(prepared.license).size > 0);
    assert.equal(prepared.archiveSha256, descriptor.archive.sha256);
    const packed = path.join(directory, descriptor.archive.name.replace(/\.tgz$/u, '-notices.txt'));
    writePdfiumNotices(prepared, packed);
    const text = fs.readFileSync(packed, 'utf8');
    assert.equal((text.match(/^===== (LICENSE|licenses\/.+) =====$/gmu) ?? []).length, 18);
    fs.rmSync(packed);
  }
});

test('rejects a changed installed loader or notice without silently replacing it', async (context) => {
  const directory = process.env.PROCYON_TEST_PDFIUM_ARCHIVES;
  if (!directory) {
    context.skip('set PROCYON_TEST_PDFIUM_ARCHIVES to the locally retained pinned archives');
    return;
  }
  const cache = fs.mkdtempSync(path.join(process.cwd(), 'target', 'pdfium-test-'));
  try {
    const descriptor = pdfiumTarget('darwin', 'arm64');
    fs.copyFileSync(
      path.join(directory, descriptor.archive.name),
      path.join(cache, descriptor.archive.name),
    );
    const prepared = await preparePinnedPdfium(descriptor, cache);
    fs.appendFileSync(prepared.library, 'changed');
    await assert.rejects(preparePinnedPdfium(descriptor, cache), /cached PDFium loader/u);
    fs.copyFileSync(
      path.join(directory, 'pdfium-mac-arm64', descriptor.loader.name),
      prepared.library,
    );
    fs.writeFileSync(path.join(prepared.notices, 'pdfium.txt'), 'changed');
    await assert.rejects(preparePinnedPdfium(descriptor, cache), /cached PDFium notice/u);
  } finally {
    fs.rmSync(cache, { recursive: true, force: true });
  }
});
