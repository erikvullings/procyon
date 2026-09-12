import { createHash } from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const MAX_CAPTURE_DURATION_MS = 15 * 60 * 1000;
const SAFE_CATEGORY = /^[a-z0-9]+(?:-[a-z0-9]+)*$/u;

function sha256(bytes) {
  return createHash('sha256').update(bytes).digest('hex');
}

function relativeEvidencePath(root, candidate) {
  const relative = path.relative(root, candidate);
  if (
    relative === '' ||
    relative === '..' ||
    relative.startsWith(`..${path.sep}`) ||
    path.isAbsolute(relative)
  ) {
    throw new Error(`evidence path escapes the qualification root: ${candidate}`);
  }
  return relative.split(path.sep).join('/');
}

function capturePath(root, value) {
  if (typeof value !== 'string' || value.length === 0 || path.isAbsolute(value)) {
    throw new Error('diagnostic capture path must be a non-empty relative path');
  }
  return path.join(root, value);
}

function collectEvidenceFiles(root) {
  const output = [];
  function visit(directory) {
    for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
      const candidate = path.join(directory, entry.name);
      if (entry.isSymbolicLink()) {
        throw new Error(
          `evidence contains a symbolic link: ${relativeEvidencePath(root, candidate)}`,
        );
      }
      if (entry.isDirectory()) visit(candidate);
      else if (entry.isFile()) output.push(candidate);
      else {
        throw new Error(
          `evidence contains an unsupported file type: ${relativeEvidencePath(root, candidate)}`,
        );
      }
    }
  }
  visit(root);
  return output.sort((left, right) =>
    relativeEvidencePath(root, left).localeCompare(relativeEvidencePath(root, right)),
  );
}

function utf16be(value) {
  const littleEndian = Buffer.from(value, 'utf16le');
  for (let index = 0; index < littleEndian.length; index += 2) {
    const first = littleEndian[index];
    littleEndian[index] = littleEndian[index + 1];
    littleEndian[index + 1] = first;
  }
  return littleEndian;
}

function canaryVariants(value) {
  const slash = value.replaceAll('\\', '/');
  const backslash = value.replaceAll('/', '\\');
  const textVariants = new Map([
    ['utf8', value],
    ['slash-path', slash],
    ['backslash-path', backslash],
    ['percent-encoded', encodeURIComponent(value)],
    ['percent-encoded-slash-path', encodeURIComponent(slash)],
    ['percent-encoded-backslash-path', encodeURIComponent(backslash)],
    ['json-escaped', JSON.stringify(value).slice(1, -1)],
    ['base64', Buffer.from(value).toString('base64')],
    ['base64url', Buffer.from(value).toString('base64url')],
    ['hex-lower', Buffer.from(value).toString('hex')],
    ['hex-upper', Buffer.from(value).toString('hex').toUpperCase()],
  ]);
  const variants = [
    ...[...textVariants].map(([encoding, text]) => ({
      encoding,
      bytes: Buffer.from(text),
    })),
    { encoding: 'utf16le', bytes: Buffer.from(value, 'utf16le') },
    { encoding: 'utf16be', bytes: utf16be(value) },
  ];
  const unique = new Map();
  for (const variant of variants) {
    if (variant.bytes.length > 0 && !unique.has(variant.bytes.toString('hex'))) {
      unique.set(variant.bytes.toString('hex'), variant);
    }
  }
  return [...unique.values()];
}

function validateCanaries(canaries) {
  if (!Array.isArray(canaries) || canaries.length === 0) {
    throw new Error('at least one privacy canary is required');
  }
  const categories = new Set();
  return canaries.map((entry) => {
    if (
      typeof entry?.category !== 'string' ||
      !SAFE_CATEGORY.test(entry.category) ||
      categories.has(entry.category)
    ) {
      throw new Error(`invalid canary category: ${entry?.category ?? '<missing>'}`);
    }
    if (typeof entry.value !== 'string' || Buffer.byteLength(entry.value) < 8) {
      throw new Error(`canary ${entry.category} must contain at least eight bytes`);
    }
    categories.add(entry.category);
    return {
      category: entry.category,
      fingerprint: sha256(`${entry.category}\0${entry.value}`),
      variants: canaryVariants(entry.value),
    };
  });
}

function validateCaptures(root, captures, now, categories) {
  const seen = new Set();
  return (captures ?? []).map((capture) => {
    const absolutePath = capturePath(root, capture.path);
    const relativePath = relativeEvidencePath(root, absolutePath);
    if (seen.has(relativePath)) {
      throw new Error(`duplicate diagnostic capture path: ${relativePath}`);
    }
    seen.add(relativePath);
    const previewedAt = new Date(capture.previewedAt);
    const expiresAt = new Date(capture.expiresAt);
    const scopedCategories = Array.isArray(capture.categories) ? capture.categories : [];
    const validTimes =
      Number.isFinite(previewedAt.valueOf()) &&
      Number.isFinite(expiresAt.valueOf()) &&
      expiresAt > previewedAt &&
      expiresAt.valueOf() - previewedAt.valueOf() <= MAX_CAPTURE_DURATION_MS &&
      now >= previewedAt &&
      now < expiresAt;
    const validCategories =
      scopedCategories.length > 0 &&
      scopedCategories.every(
        (category) =>
          typeof category === 'string' && SAFE_CATEGORY.test(category) && categories.has(category),
      );
    return {
      absolutePath,
      relativePath,
      categories: new Set(scopedCategories),
      previewedAt: Number.isFinite(previewedAt.valueOf()) ? previewedAt.toISOString() : null,
      expiresAt: Number.isFinite(expiresAt.valueOf()) ? expiresAt.toISOString() : null,
      valid: validTimes && validCategories,
      authorizedFingerprints: new Set(),
      removed: false,
    };
  });
}

async function inspectFile(file, needles, chunkBytes) {
  const digest = createHash('sha256');
  const findings = [];
  const maximumNeedleBytes = Math.max(
    1,
    ...needles.flatMap((canary) => canary.variants.map((variant) => variant.bytes.length)),
  );
  let carry = Buffer.alloc(0);
  let consumed = 0;
  const matches = new Set();
  const stream = fs.createReadStream(file, { highWaterMark: chunkBytes });
  for await (const chunk of stream) {
    digest.update(chunk);
    const bytes = Buffer.concat([carry, chunk]);
    const baseOffset = consumed - carry.length;
    for (const canary of needles) {
      for (const variant of canary.variants) {
        let index = bytes.indexOf(variant.bytes);
        while (index >= 0) {
          const offset = baseOffset + index;
          const identity = `${canary.category}\0${offset}`;
          if (!matches.has(identity)) {
            matches.add(identity);
            findings.push({
              category: canary.category,
              canarySha256: canary.fingerprint,
              encoding: variant.encoding,
              offset,
            });
          }
          index = bytes.indexOf(variant.bytes, index + 1);
        }
      }
    }
    consumed += chunk.length;
    carry = bytes.subarray(Math.max(0, bytes.length - maximumNeedleBytes + 1));
  }
  return {
    bytes: consumed,
    sha256: digest.digest('hex'),
    findings,
  };
}

/**
 * Scans retained qualification evidence without emitting sensitive canary values.
 *
 * @param {{
 *   root: string,
 *   canaries: Array<{category: string, value: string}>,
 *   captures?: Array<{
 *     path: string,
 *     categories: string[],
 *     previewedAt: string,
 *     expiresAt: string
 *   }>,
 *   now?: Date,
 *   chunkBytes?: number
 * }} input
 */
export async function scanSemanticPrivacyEvidence(input) {
  const root = path.resolve(input.root);
  const stat = fs.statSync(root);
  if (!stat.isDirectory()) throw new Error('privacy evidence root must be a directory');
  const now = input.now ?? new Date();
  if (!(now instanceof Date) || !Number.isFinite(now.valueOf())) {
    throw new Error('privacy scan time must be a valid Date');
  }
  const chunkBytes = input.chunkBytes ?? 64 * 1024;
  if (!Number.isSafeInteger(chunkBytes) || chunkBytes < 1) {
    throw new Error('privacy scan chunk size must be a positive safe integer');
  }
  const canaries = validateCanaries(input.canaries);
  const categories = new Set(canaries.map(({ category }) => category));
  const captures = validateCaptures(root, input.captures, now, categories);
  const capturesByPath = new Map(captures.map((capture) => [capture.relativePath, capture]));
  const evidenceFiles = [];
  const findings = [];

  try {
    for (const file of collectEvidenceFiles(root)) {
      const relativePath = relativeEvidencePath(root, file);
      const inspected = await inspectFile(file, canaries, chunkBytes);
      const capture = capturesByPath.get(relativePath);
      let leaked = false;
      for (const finding of inspected.findings) {
        if (capture?.valid && capture.categories.has(finding.category)) {
          capture.authorizedFingerprints.add(finding.canarySha256);
        } else {
          leaked = true;
          findings.push({ file: relativePath, removed: true, ...finding });
        }
      }
      if (leaked) {
        fs.rmSync(file);
      } else if (!capture) {
        evidenceFiles.push({
          path: relativePath,
          bytes: inspected.bytes,
          sha256: inspected.sha256,
        });
      }
    }
  } finally {
    for (const capture of captures) {
      if (fs.existsSync(capture.absolutePath)) {
        fs.rmSync(capture.absolutePath, { force: true });
      }
      capture.removed = !fs.existsSync(capture.absolutePath);
    }
  }

  return {
    schemaVersion: 1,
    scanner: 'procyon-semantic-privacy-v1',
    scannedAt: now.toISOString(),
    status: findings.length === 0 ? 'pass' : 'fail',
    canaryCategories: canaries.map(({ category, fingerprint }) => ({
      category,
      canarySha256: fingerprint,
    })),
    findings,
    authorizedCaptures: captures.map((capture) => ({
      path: capture.relativePath,
      categories: [...capture.categories].sort(),
      previewedAt: capture.previewedAt,
      expiresAt: capture.expiresAt,
      validAtScan: capture.valid,
      canarySha256: [...capture.authorizedFingerprints].sort(),
      removed: capture.removed,
    })),
    evidenceFiles: evidenceFiles.sort((left, right) => left.path.localeCompare(right.path)),
  };
}

/**
 * Re-hashes a retained evidence tree and rejects changes after the privacy scan.
 *
 * @param {{root: string, report: Awaited<ReturnType<typeof scanSemanticPrivacyEvidence>>}} input
 */
export async function verifySemanticPrivacyEvidence({ root: rootInput, report }) {
  const root = path.resolve(rootInput);
  const expected = new Map(report.evidenceFiles.map((file) => [file.path, file]));
  const actualFiles = collectEvidenceFiles(root);
  for (const file of actualFiles) {
    const relativePath = relativeEvidencePath(root, file);
    if (!expected.has(relativePath)) {
      throw new Error(`unexpected evidence file after privacy scan: ${relativePath}`);
    }
  }
  for (const expectedFile of report.evidenceFiles) {
    const file = path.join(root, expectedFile.path);
    if (!fs.existsSync(file)) {
      throw new Error(`evidence file is missing after privacy scan: ${expectedFile.path}`);
    }
    const bytes = fs.readFileSync(file);
    if (bytes.length !== expectedFile.bytes || sha256(bytes) !== expectedFile.sha256) {
      throw new Error(`evidence digest changed after privacy scan: ${expectedFile.path}`);
    }
  }
}

async function cli() {
  const [root, canariesPath, reportPath] = process.argv.slice(2);
  if (!root || !canariesPath || !reportPath) {
    throw new Error(
      'usage: semantic-privacy-scan.mjs <evidence-root> <private-canaries.json> <report.json>',
    );
  }
  const input = JSON.parse(fs.readFileSync(canariesPath, 'utf8'));
  const report = await scanSemanticPrivacyEvidence({
    root,
    canaries: input.canaries,
    captures: input.captures,
  });
  fs.writeFileSync(reportPath, `${JSON.stringify(report, null, 2)}\n`, { flag: 'wx' });
  await verifySemanticPrivacyEvidence({ root, report });
  if (report.status !== 'pass') {
    throw new Error(
      `privacy scan found ${report.findings.length} canary occurrence(s); see the redacted report`,
    );
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  cli().catch((error) => {
    console.error(error.message);
    process.exitCode = 1;
  });
}
