import { createHash } from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { deflateSync } from 'node:zlib';

function pagePixels(width, height, page) {
  const pixels = Buffer.alloc(width * height * 3);
  let state = (0x91e10da5 ^ Math.imul(page, 0x9e3779b9)) >>> 0;
  for (let index = 0; index < pixels.length; index++) {
    state ^= state << 13;
    state ^= state >>> 17;
    state ^= state << 5;
    pixels[index] = state & 0xff;
  }
  return pixels;
}

export function generateSemanticPdfFixture(filename, { pages, width, height, textLayer }) {
  if (
    !Number.isSafeInteger(pages) ||
    pages < 1 ||
    pages > 354 ||
    !Number.isSafeInteger(width) ||
    width < 1 ||
    !Number.isSafeInteger(height) ||
    height < 1 ||
    width * height * 3 > 1024 * 1024
  ) {
    throw new Error('fixture page count and image dimensions must be positive and bounded');
  }
  if (fs.existsSync(filename)) {
    throw new Error(`fixture already exists: ${filename}`);
  }
  const temporary = `${filename}.part`;
  const fd = fs.openSync(temporary, 'wx', 0o600);
  let open = true;
  let position = 0;
  const offsets = [0];
  const write = (bytes) => {
    const buffer = typeof bytes === 'string' ? Buffer.from(bytes) : bytes;
    let written = 0;
    while (written < buffer.length) {
      written += fs.writeSync(fd, buffer, written, buffer.length - written);
    }
    position += buffer.length;
  };
  const object = (id, body) => {
    offsets[id] = position;
    write(`${id} 0 obj\n`);
    write(body);
    write('\nendobj\n');
  };

  try {
    write('%PDF-1.4\n');
    object(1, '<< /Type /Catalog /Pages 2 0 R >>');
    const kids = Array.from({ length: pages }, (_, index) => `${4 + 3 * index} 0 R`).join(' ');
    object(2, `<< /Type /Pages /Kids [${kids}] /Count ${pages} >>`);
    object(3, '<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>');
    for (let page = 1; page <= pages; page++) {
      const pageId = 4 + 3 * (page - 1);
      object(
        pageId,
        `<< /Type /Page /Parent 2 0 R /MediaBox [0 0 ${width} ${height}] /Resources << /XObject << /Im0 ${pageId + 2} 0 R >> /Font << /F1 3 0 R >> >> /Contents ${pageId + 1} 0 R >>`,
      );
      const label = textLayer
        ? `BT /F1 12 Tf 12 24 Td (Synthetic page ${page} of ${pages}: indexed text evidence) Tj ET\n`
        : '';
      const drawing = `q ${width} 0 0 ${height} 0 0 cm /Im0 Do Q\n${label}`;
      object(
        pageId + 1,
        `<< /Length ${Buffer.byteLength(drawing)} >>\nstream\n${drawing}endstream`,
      );
      const compressed = deflateSync(pagePixels(width, height, page), { level: 1 });
      offsets[pageId + 2] = position;
      write(`${pageId + 2} 0 obj\n`);
      write(
        `<< /Type /XObject /Subtype /Image /Width ${width} /Height ${height} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode /Length ${compressed.length} >>\nstream\n`,
      );
      write(compressed);
      write('\nendstream\nendobj\n');
    }
    const xref = position;
    write(`xref\n0 ${offsets.length}\n0000000000 65535 f \n`);
    for (const offset of offsets.slice(1)) {
      write(`${String(offset).padStart(10, '0')} 00000 n \n`);
    }
    write(`trailer\n<< /Size ${offsets.length} /Root 1 0 R >>\nstartxref\n${xref}\n%%EOF\n`);
    fs.closeSync(fd);
    open = false;
    fs.linkSync(temporary, filename);
    fs.rmSync(temporary);
    return { pages, bytes: position };
  } catch (error) {
    if (open) fs.closeSync(fd);
    fs.rmSync(temporary, { force: true });
    throw error;
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const directory = process.argv[2];
  if (!directory || !fs.existsSync(directory) || !fs.statSync(directory).isDirectory()) {
    throw new Error('pass an existing output directory; fixtures are never written to the repo');
  }
  for (const [name, height, textLayer] of [
    ['synthetic-354-page-scan.pdf', 409, false],
    ['synthetic-354-page-text.pdf', 444, true],
  ]) {
    const filename = path.join(directory, name);
    const result = generateSemanticPdfFixture(filename, {
      pages: 354,
      width: 400,
      height,
      textLayer,
    });
    const digest = createHash('sha256');
    const file = fs.openSync(filename, 'r');
    const buffer = Buffer.alloc(1024 * 1024);
    try {
      for (let length = fs.readSync(file, buffer); length > 0; length = fs.readSync(file, buffer)) {
        digest.update(buffer.subarray(0, length));
      }
    } finally {
      fs.closeSync(file);
    }
    console.log(
      `${name}: ${result.pages} pages, ${result.bytes} bytes, SHA-256 ${digest.digest('hex')}`,
    );
  }
}
