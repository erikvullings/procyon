import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import fs from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import {
  pdfiumTarget,
  preparePinnedPdfium,
  writePdfiumNotices,
} from './pdfium-runtime-qualification.mjs';
import { sha256Of, verifyMacDeveloperIdSignature } from './zvec-runtime-qualification.mjs';

const repositoryRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const PDFIUM_COMPONENT = 'procyon.semantic.pdfium-runtime';
const NOTICES_COMPONENT = 'procyon.semantic.pdfium-notices';

function verifiedArtifact(bundle, artifact) {
  const file = path.join(bundle, 'artifacts', artifact.id);
  const checksum = Buffer.from(artifact.checksum ?? []).toString('hex');
  if (
    !fs.existsSync(file) ||
    !fs.statSync(file).isFile() ||
    fs.statSync(file).size !== artifact.resources.download_bytes ||
    sha256Of(file) !== checksum
  ) {
    throw new Error(`packaged PDFium artifact ${artifact.id} differs from catalog input`);
  }
  return file;
}

export function parseRenderedPdfPage(report) {
  let offset = 0;
  function number() {
    if (offset + 4 > report.length) throw new Error('PDFium page report is incomplete');
    const value = report.readUInt32BE(offset);
    offset += 4;
    return value;
  }
  if (number() !== 1 || number() !== 1) {
    throw new Error('PDFium smoke expected one rendered page and one image');
  }
  const headerLength = number();
  if (headerLength === 0 || headerLength > 4092 || offset + headerLength > report.length) {
    throw new Error('PDFium page header exceeds the frame budget');
  }
  const header = JSON.parse(report.subarray(offset, offset + headerLength).toString('utf8'));
  offset += headerLength;
  if (
    header.page_number !== 1 ||
    header.image_index !== 0 ||
    header.rendered_page !== true ||
    header.media_type !== 'image/jpeg' ||
    !Number.isInteger(header.width) ||
    !Number.isInteger(header.height) ||
    header.width < 1 ||
    header.height < 1 ||
    header.width * header.height > 4_000_000 ||
    !Number.isInteger(header.byte_length) ||
    header.byte_length < 1 ||
    header.byte_length > 4 * 1024 * 1024 ||
    offset + header.byte_length !== report.length ||
    report[offset] !== 0xff ||
    report[offset + 1] !== 0xd8
  ) {
    throw new Error('PDFium smoke result lacks bounded page-rendered JPEG evidence');
  }
  return { width: header.width, height: header.height, bytes: header.byte_length };
}

export async function smokePackagedPdfiumRenderer(
  bundle,
  manifest,
  isolatedNativeDirectory,
  runtimeLoaderEnvironment,
  cacheRoot,
) {
  const artifacts = manifest.catalog.artifacts;
  const pdfium = artifacts.filter((item) => item.component_id === PDFIUM_COMPONENT);
  const notices = artifacts.filter((item) => item.component_id === NOTICES_COMPONENT);
  if (pdfium.length === 0 && notices.length === 0) return undefined;
  if (pdfium.length !== 1 || notices.length !== 1) {
    throw new Error('packaged PDFium loader and license notices must both be present');
  }
  const descriptor = pdfiumTarget();
  const library = verifiedArtifact(bundle, pdfium[0]);
  const licenseArtifact = verifiedArtifact(bundle, notices[0]);
  const root = fs.mkdtempSync(path.join(tmpdir(), 'procyon-pdfium-smoke-'));
  try {
    const prepared = await preparePinnedPdfium(descriptor, cacheRoot);
    const expectedNotices = writePdfiumNotices(prepared, path.join(root, 'expected-notices.txt'));
    if (!fs.readFileSync(licenseArtifact).equals(fs.readFileSync(expectedNotices))) {
      throw new Error('signed PDFium notices differ from the complete pinned upstream terms');
    }
    if (process.platform === 'darwin') {
      verifyMacDeveloperIdSignature(library);
    } else if (sha256Of(library) !== descriptor.loader.sha256) {
      throw new Error('signed PDFium loader differs from the pinned native bytes');
    }
    const workerArtifact = artifacts.find(
      (item) => item.component_id === 'procyon.semantic.worker',
    );
    if (!workerArtifact) throw new Error('packaged PDFium has no worker');
    const worker = path.join(
      isolatedNativeDirectory,
      process.platform === 'win32' ? 'fm-semantic-worker.exe' : 'fm-semantic-worker',
    );
    fs.copyFileSync(verifiedArtifact(bundle, workerArtifact), worker);
    fs.chmodSync(worker, fs.statSync(path.join(bundle, 'artifacts', workerArtifact.id)).mode);
    fs.copyFileSync(library, path.join(isolatedNativeDirectory, descriptor.loader.name));
    fs.copyFileSync(licenseArtifact, path.join(isolatedNativeDirectory, 'pdfium-licenses.txt'));
    const fixture = path.join(root, 'one-page-rle-scan.pdf');
    const generated = spawnSync(
      process.execPath,
      [
        path.join(repositoryRoot, 'scripts', 'generate-large-pdf-fixture.mjs'),
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
      { timeout: 10_000, encoding: 'utf8' },
    );
    if (generated.error || generated.status !== 0) {
      throw generated.error ?? new Error(`synthetic PDF generation failed: ${generated.stderr}`);
    }
    const source = fs.readFileSync(fixture);
    const digest = createHash('sha256').update(source).digest('hex');
    const rendered = spawnSync(worker, ['--pdf-visual-pages-child', digest], {
      input: source,
      timeout: 15_000,
      maxBuffer: 4 * 1024 * 1024,
      env: { ...process.env, ...runtimeLoaderEnvironment },
    });
    if (rendered.error || rendered.status !== 0) {
      throw rendered.error ?? new Error('packaged PDFium worker failed synthetic page rendering');
    }
    return parseRenderedPdfPage(rendered.stdout);
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
}
