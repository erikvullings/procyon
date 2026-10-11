import { closeSync, openSync, unlinkSync, writeSync } from 'node:fs';
import { deflateSync } from 'node:zlib';

const maxSourceBytes = 200 * 1024 * 1024;
const args = process.argv.slice(2);
const options = new Map();
for (let index = 0; index < args.length; index += 2) {
  if (
    !['--output', '--variant', '--pages', '--width', '--height'].includes(args[index]) ||
    !args[index + 1] ||
    options.has(args[index])
  ) {
    throw new Error('Expected unique --output, --variant, --pages, --width, --height options');
  }
  options.set(args[index], args[index + 1]);
}
const output = options.get('--output');
const variant = options.get('--variant');
if (!output || !['scanned', 'text'].includes(variant)) {
  throw new Error('Supply --output and --variant scanned|text');
}
function dimension(name, fallback, maximum) {
  const value = options.get(name) ?? String(fallback);
  if (!/^[1-9]\d*$/.test(value) || Number(value) > maximum) {
    throw new Error(`${name} must be an integer from 1 to ${maximum}`);
  }
  return Number(value);
}
const pages = dimension('--pages', 354, 4096);
const width = dimension('--width', variant === 'text' ? 536 : 512, 2048);
const height = dimension('--height', variant === 'text' ? 332 : 320, 2048);
if (pages * width * height * 3 + pages * 4096 > maxSourceBytes) {
  throw new Error('Estimated fixture exceeds the fixed 200 MiB source bound');
}

const fd = openSync(output, 'wx', 0o600);
let offset = 0;
const locations = [0];
function write(data) {
  const bytes = Buffer.isBuffer(data) ? data : Buffer.from(data, 'latin1');
  if (offset + bytes.length > maxSourceBytes) {
    throw new Error('Generated fixture exceeds the fixed 200 MiB source bound');
  }
  let position = 0;
  while (position < bytes.length) {
    position += writeSync(fd, bytes, position, bytes.length - position);
  }
  offset += bytes.length;
}
function object(id, body) {
  locations[id] = offset;
  write(`${id} 0 obj\n`);
  write(body);
  write('\nendobj\n');
}
function stream(id, dictionary, bytes) {
  locations[id] = offset;
  write(`${id} 0 obj\n<< ${dictionary} /Length ${bytes.length} >>\nstream\n`);
  write(bytes);
  write('\nendstream\nendobj\n');
}
function image(page) {
  const pixels = Buffer.allocUnsafe(width * height * 3);
  let state = (0x9e3779b9 ^ page) >>> 0;
  for (let i = 0; i < pixels.length; i++) {
    state ^= state << 13;
    state ^= state >>> 17;
    state ^= state << 5;
    pixels[i] = state & 0xff;
  }
  return deflateSync(pixels, { level: 1 });
}
try {
  write('%PDF-1.7\n%\xe2\xe3\xcf\xd3\n');
  object(1, '<< /Type /Catalog /Pages 2 0 R >>');
  const kids = Array.from({ length: pages }, (_, i) => `${4 + i * 3} 0 R`).join(' ');
  object(2, `<< /Type /Pages /Count ${pages} /Kids [${kids}] >>`);
  object(3, '<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>');
  for (let page = 1; page <= pages; page++) {
    const id = 4 + (page - 1) * 3;
    object(
      id,
      `<< /Type /Page /Parent 2 0 R /MediaBox [0 0 ${width} ${height}] /Resources << /XObject << /Im ${id + 2} 0 R >> /Font << /F1 3 0 R >> >> /Contents ${id + 1} 0 R >>`,
    );
    const label =
      variant === 'text'
        ? `BT /F1 12 Tf 12 12 Td (Synthetic search marker on page ${page}) Tj ET\n`
        : '';
    const content = Buffer.from(`q ${width} 0 0 ${height} 0 0 cm /Im Do Q\n${label}`, 'ascii');
    stream(id + 1, '', content);
    const compressed = image(page);
    stream(
      id + 2,
      `/Type /XObject /Subtype /Image /Width ${width} /Height ${height} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode`,
      compressed,
    );
  }
  const xref = offset;
  write(`xref\n0 ${locations.length}\n0000000000 65535 f \n`);
  for (const location of locations.slice(1)) {
    write(`${String(location).padStart(10, '0')} 00000 n \n`);
  }
  write(`trailer\n<< /Size ${locations.length} /Root 1 0 R >>\nstartxref\n${xref}\n%%EOF\n`);
  console.log(`Synthetic ${variant} PDF: ${pages} pages, ${offset} bytes`);
} catch (error) {
  closeSync(fd);
  unlinkSync(output);
  throw error;
}
closeSync(fd);
