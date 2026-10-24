import type { DiffRow, DiffSegment, FileDiffOptions, FileDiffResult } from '../../models';

const SIZE_LIMIT_BYTES = 2 * 1024 * 1024;
const LINE_LIMIT = 100_000;

function lineKey(line: string, options: FileDiffOptions): string {
  let key = options.ignoreWhitespace === true ? line.split(/\s+/).filter(Boolean).join(' ') : line;
  if (options.ignoreCase === true) key = key.toLowerCase();
  return key;
}

/** Marks everything outside the common prefix and suffix of the two lines as changed. */
function segments(line: string, other: string): DiffSegment[] {
  let prefix = 0;
  while (prefix < line.length && prefix < other.length && line[prefix] === other[prefix]) prefix++;
  let suffix = 0;
  while (
    suffix < line.length - prefix &&
    suffix < other.length - prefix &&
    line[line.length - 1 - suffix] === other[other.length - 1 - suffix]
  ) {
    suffix++;
  }
  return [
    { text: line.slice(0, prefix), changed: false },
    { text: line.slice(prefix, line.length - suffix), changed: true },
    { text: line.slice(line.length - suffix), changed: false },
  ].filter((segment) => segment.text.length > 0);
}

function splitLines(bytes: Uint8Array): string[] {
  const text = new TextDecoder().decode(bytes);
  if (text.length === 0) return [];
  const lines = text.split(/\r?\n/);
  if (lines[lines.length - 1] === '') lines.pop();
  return lines;
}

/** Line diff of the in-memory mock fixtures, mirroring the shape the backend returns (task 0256). */
export function mockFileDiff(
  left: Uint8Array,
  right: Uint8Array,
  options: FileDiffOptions = {},
): FileDiffResult {
  const base = {
    leftSize: left.length,
    rightSize: right.length,
    sizeLimitBytes: SIZE_LIMIT_BYTES,
    lineLimit: LINE_LIMIT,
    rows: [] as DiffRow[],
    changeStarts: [] as number[],
  };
  if (left.includes(0) || right.includes(0)) {
    const common = Math.min(left.length, right.length);
    let offset = left.findIndex((byte, index) => index < common && byte !== right[index]);
    if (offset < 0 && left.length !== right.length) offset = common;
    return {
      ...base,
      outcome: 'binary',
      identical: offset < 0,
      firstDifferenceOffset: offset < 0 ? null : offset,
    };
  }
  if (left.length > SIZE_LIMIT_BYTES || right.length > SIZE_LIMIT_BYTES) {
    return { ...base, outcome: 'tooLarge', identical: false };
  }
  const a = splitLines(left);
  const b = splitLines(right);
  const ka = a.map((line) => lineKey(line, options));
  const kb = b.map((line) => lineKey(line, options));
  // Longest-common-subsequence table; the mock fixtures are tiny.
  const table: number[][] = Array.from({ length: a.length + 1 }, () =>
    new Array<number>(b.length + 1).fill(0),
  );
  const cell = (i: number, j: number): number => table[i]?.[j] ?? 0;
  for (let i = a.length - 1; i >= 0; i--) {
    const row = table[i];
    if (row === undefined) continue;
    for (let j = b.length - 1; j >= 0; j--) {
      row[j] = ka[i] === kb[j] ? cell(i + 1, j + 1) + 1 : Math.max(cell(i + 1, j), cell(i, j + 1));
    }
  }

  const rows: DiffRow[] = [];
  const changeStarts: number[] = [];
  let i = 0;
  let j = 0;
  while (i < a.length || j < b.length) {
    if (i < a.length && j < b.length && ka[i] === kb[j]) {
      rows.push({
        kind: 'equal',
        leftLine: i + 1,
        rightLine: j + 1,
        leftText: a[i] ?? '',
        rightText: b[j] ?? '',
        leftSegments: [],
        rightSegments: [],
      });
      i++;
      j++;
      continue;
    }
    // A change block: collect the deleted and inserted lines up to the next common line.
    const blockStart = rows.length;
    const deleted: number[] = [];
    const inserted: number[] = [];
    while ((i < a.length || j < b.length) && !(i < a.length && j < b.length && ka[i] === kb[j])) {
      if (j >= b.length || (i < a.length && cell(i + 1, j) >= cell(i, j + 1))) {
        deleted.push(i++);
      } else {
        inserted.push(j++);
      }
    }
    changeStarts.push(blockStart);
    const paired = Math.min(deleted.length, inserted.length);
    for (let k = 0; k < paired; k++) {
      const leftIndex = deleted[k] ?? 0;
      const rightIndex = inserted[k] ?? 0;
      const leftText = a[leftIndex] ?? '';
      const rightText = b[rightIndex] ?? '';
      rows.push({
        kind: 'replace',
        leftLine: leftIndex + 1,
        rightLine: rightIndex + 1,
        leftText,
        rightText,
        leftSegments: segments(leftText, rightText),
        rightSegments: segments(rightText, leftText),
      });
    }
    for (const leftIndex of deleted.slice(paired)) {
      rows.push({
        kind: 'delete',
        leftLine: leftIndex + 1,
        leftText: a[leftIndex] ?? '',
        leftSegments: [],
        rightSegments: [],
      });
    }
    for (const rightIndex of inserted.slice(paired)) {
      rows.push({
        kind: 'insert',
        rightLine: rightIndex + 1,
        rightText: b[rightIndex] ?? '',
        leftSegments: [],
        rightSegments: [],
      });
    }
  }
  return {
    ...base,
    outcome: 'text',
    identical: changeStarts.length === 0,
    rows,
    changeStarts,
  };
}
