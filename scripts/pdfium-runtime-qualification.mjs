import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';

import { verifyPinnedFile } from './zvec-runtime-qualification.mjs';

export const PDFIUM_REVISION = 'chromium/8086';
const releaseBase = `https://github.com/bblanchon/pdfium-binaries/releases/download/${PDFIUM_REVISION}`;

const targets = new Map([
  [
    'darwin-arm64',
    {
      archive: {
        name: 'pdfium-mac-arm64.tgz',
        bytes: 3521983,
        sha256: 'e98679e052c07edbb5a627980902abb823d4b3f35744d877bd21668bd9fc13ab',
      },
      loader: {
        name: 'libpdfium.dylib',
        member: 'lib/libpdfium.dylib',
        bytes: 7339520,
        sha256: 'f49cd07cefc0aa4978650fc3cd9d2086fd8d3ca5fcbb2b13e7454df0356be014',
        machine: 0x0100000c,
      },
    },
  ],
  [
    'linux-arm64',
    {
      archive: {
        name: 'pdfium-linux-arm64.tgz',
        bytes: 3708875,
        sha256: 'e7e2fe4686925618330103cb167950aca5a84bb00fd977a41b86be59dd1480a2',
      },
      loader: {
        name: 'libpdfium.so',
        member: 'lib/libpdfium.so',
        bytes: 8075568,
        sha256: '73786a7c9dc54f68bc1877a4327d1ecde7bd62500601ee7db558041fc0c7aa2b',
        machine: 183,
      },
    },
  ],
  [
    'linux-x64',
    {
      archive: {
        name: 'pdfium-linux-x64.tgz',
        bytes: 3788766,
        sha256: '588577cf52dabc1a444988bac841920df54cc2f141801424de97ab04f4fbb935',
      },
      loader: {
        name: 'libpdfium.so',
        member: 'lib/libpdfium.so',
        bytes: 7942208,
        sha256: 'e995a89f11d48605a79a0e44f101ca3a621e5c3ef9ee311a1405e03b44dc5b19',
        machine: 62,
      },
    },
  ],
  [
    'win32-x64',
    {
      archive: {
        name: 'pdfium-win-x64.tgz',
        bytes: 3866531,
        sha256: '1fd8af952832dbb0eb16d9249f68fe09e5f5ebf7c3dd9f6066ea2720cc28487d',
      },
      loader: {
        name: 'pdfium.dll',
        member: 'bin/pdfium.dll',
        bytes: 7494656,
        sha256: '1f2aee4286b8cf0413ab4562b7121aa454f9cfd9afd6ca8609bcef80927b3f80',
        machine: 0x8664,
      },
    },
  ],
]);

const noticeNames = [
  'abseil.txt',
  'agg23.txt',
  'dragonbox-Apache2-LLVM.txt',
  'dragonbox-Boost.txt',
  'fast_float.txt',
  'freetype.txt',
  'harfbuzz.txt',
  'icu.txt',
  'lcms.txt',
  'libjpeg_turbo.ijg',
  'libjpeg_turbo.md',
  'libopenjpeg.txt',
  'libpng.txt',
  'llvm-libc.txt',
  'pdfium.txt',
  'simdutf.txt',
  'zlib.txt',
];

export function pdfiumTarget(platform = process.platform, architecture = process.arch) {
  const descriptor = targets.get(`${platform}-${architecture}`);
  if (!descriptor) throw new Error(`No pinned PDFium for ${platform}-${architecture}`);
  return descriptor;
}

function tar(archive, args, options = {}) {
  return execFileSync('tar', [...args, archive], { maxBuffer: 16 * 1024 * 1024, ...options });
}

function memberBytes(archive, member) {
  return execFileSync('tar', ['-xOzf', archive, member], { maxBuffer: 16 * 1024 * 1024 });
}

export function verifyPdfiumMembers(members, descriptor) {
  const unique = new Set(members.filter(Boolean));
  if (
    unique.size !== members.filter(Boolean).length ||
    members.some((member) => member.startsWith('/') || member.split('/').includes('..'))
  ) {
    throw new Error('PDFium archive contains an unsafe or duplicate member');
  }
  for (const name of [
    'LICENSE',
    'VERSION',
    descriptor.loader.member,
    ...noticeNames.map((notice) => `licenses/${notice}`),
  ]) {
    if (!unique.has(name)) throw new Error(`PDFium archive is missing ${name}`);
  }
  if (unique.has('bin/pdfium.dll') && descriptor.loader.name !== 'pdfium.dll') {
    throw new Error('PDFium archive contains a DLL on a non-Windows target');
  }
}

function verifyBytes(data, pinned, label) {
  const digest = createHash('sha256').update(data).digest('hex');
  if (data.length !== pinned.bytes || digest !== pinned.sha256) {
    throw new Error(`${label} differs from pinned PDFium bytes`);
  }
}

function verifyLoaderArchitecture(data, descriptor) {
  const { name, machine } = descriptor.loader;
  let actual;
  if (name.endsWith('.dylib') && data.readUInt32LE(0) === 0xfeedfacf) {
    actual = data.readUInt32LE(4);
  } else if (
    name.endsWith('.so') &&
    data.subarray(0, 6).equals(Buffer.from([0x7f, 0x45, 0x4c, 0x46, 2, 1]))
  ) {
    actual = data.readUInt16LE(18);
  } else if (name.endsWith('.dll') && data.toString('ascii', 0, 2) === 'MZ') {
    const offset = data.readUInt32LE(0x3c);
    if (offset + 6 <= data.length && data.toString('ascii', offset, offset + 4) === 'PE\0\0') {
      actual = data.readUInt16LE(offset + 4);
    }
  }
  if (actual !== machine) throw new Error('PDFium loader architecture differs from target');
}

export function verifyPdfiumArchive(archive, descriptor) {
  verifyPinnedFile(archive, descriptor.archive, 'pinned PDFium archive');
  const members = tar(archive, ['-tzf']).toString('utf8').trim().split(/\r?\n/u);
  verifyPdfiumMembers(members, descriptor);
  if (!memberBytes(archive, 'VERSION').toString('utf8').includes('MAJOR=157\n')) {
    throw new Error('PDFium archive version differs from chromium/8086');
  }
  const loader = memberBytes(archive, descriptor.loader.member);
  verifyBytes(loader, descriptor.loader, 'PDFium loader');
  verifyLoaderArchitecture(loader, descriptor);
  const notices = new Map();
  for (const name of ['LICENSE', ...noticeNames.map((notice) => `licenses/${notice}`)]) {
    const content = memberBytes(archive, name);
    if (content.length === 0 || content.length > 1024 * 1024) {
      throw new Error(`PDFium notice ${name} is empty or exceeds the notice budget`);
    }
    notices.set(name, content);
  }
  return { loader, notices };
}

async function downloadArchive(url, destination, expected) {
  const response = await fetch(url, { redirect: 'follow' });
  if (!response.ok || !response.body)
    throw new Error(`PDFium download returned ${response.status}`);
  const temporary = `${destination}.partial`;
  let handle;
  try {
    handle = await fs.promises.open(temporary, 'w');
    let length = 0;
    for await (const chunk of response.body) {
      length += chunk.length;
      if (length > expected.bytes) throw new Error('PDFium download exceeds pinned size');
      await handle.write(chunk);
    }
    await handle.close();
    handle = undefined;
    verifyPinnedFile(temporary, expected, 'downloaded PDFium archive');
    fs.renameSync(temporary, destination);
  } catch (error) {
    await handle?.close();
    fs.rmSync(temporary, { force: true });
    throw error;
  }
}

export async function preparePinnedPdfium(descriptor, cacheRoot) {
  const root = path.resolve(cacheRoot);
  fs.mkdirSync(root, { recursive: true });
  const archive = path.join(root, descriptor.archive.name);
  if (!fs.existsSync(archive)) {
    await downloadArchive(`${releaseBase}/${descriptor.archive.name}`, archive, descriptor.archive);
  }
  const { loader, notices } = verifyPdfiumArchive(archive, descriptor);
  const installed = path.join(root, descriptor.archive.name.replace(/\.tgz$/u, ''));
  const result = {
    library: path.join(installed, descriptor.loader.name),
    notices: path.join(installed, 'licenses'),
    license: path.join(installed, 'LICENSE'),
    archiveSha256: descriptor.archive.sha256,
  };
  if (fs.existsSync(installed)) {
    verifyPinnedFile(result.library, descriptor.loader, 'cached PDFium loader');
    for (const [name, content] of notices) {
      const cached = path.join(installed, name);
      if (!fs.existsSync(cached) || !fs.readFileSync(cached).equals(content)) {
        throw new Error(`cached PDFium notice ${name} differs from pinned archive`);
      }
    }

    return result;
  }

  const staging = fs.mkdtempSync(path.join(root, 'pdfium-extract-'));
  try {
    fs.writeFileSync(path.join(staging, descriptor.loader.name), loader);
    for (const [name, content] of notices) {
      const destination = path.join(staging, name);
      fs.mkdirSync(path.dirname(destination), { recursive: true });
      fs.writeFileSync(destination, content);
    }
    fs.renameSync(staging, installed);
    return result;
  } finally {
    if (fs.existsSync(staging)) fs.rmSync(staging, { recursive: true, force: true });
  }
}

export function writePdfiumNotices(prepared, destination) {
  const entries = [
    ['LICENSE', prepared.license],
    ...noticeNames.map((name) => [`licenses/${name}`, path.join(prepared.notices, name)]),
  ];
  const content = [];
  for (const [name, source] of entries) {
    const bytes = fs.readFileSync(source);
    if (bytes.length === 0 || bytes.length > 1024 * 1024) {
      throw new Error(`PDFium notice ${name} is empty or exceeds the notice budget`);
    }
    content.push(Buffer.from(`\n===== ${name} =====\n`), bytes, Buffer.from('\n'));
  }
  fs.writeFileSync(destination, Buffer.concat(content));
  return destination;
}
