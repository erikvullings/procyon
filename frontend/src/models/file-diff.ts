/** Ignore options of a two-file diff (task 0256). */
export interface FileDiffOptions {
  /** Treat lines that differ only in whitespace as equal. */
  ignoreWhitespace?: boolean;
  /** Treat lines that differ only in letter case as equal. */
  ignoreCase?: boolean;
}

/** How a pair of files was compared. */
export type FileDiffOutcome = 'text' | 'binary' | 'tooLarge';

/** Role of one aligned row of the side-by-side diff. */
export type DiffRowKind = 'equal' | 'delete' | 'insert' | 'replace';

/** A run of characters inside a changed line. */
export interface DiffSegment {
  text: string;
  changed: boolean;
}

/** One aligned row of the side-by-side diff. */
export interface DiffRow {
  kind: DiffRowKind;
  leftLine?: number | null;
  rightLine?: number | null;
  leftText?: string | null;
  rightText?: string | null;
  /** Intra-line runs of a replaced left line; empty for every other row. */
  leftSegments: DiffSegment[];
  /** Intra-line runs of a replaced right line; empty for every other row. */
  rightSegments: DiffSegment[];
}

/** The result of diffing two files. */
export interface FileDiffResult {
  outcome: FileDiffOutcome;
  leftSize: number;
  rightSize: number;
  /** Identical under the requested ignore options (byte-exact for binary files). */
  identical: boolean;
  /** Binary files only: zero-based offset of the first differing byte. */
  firstDifferenceOffset?: number | null;
  sizeLimitBytes: number;
  lineLimit: number;
  /** Text diff only: the aligned rows, in display order. */
  rows: DiffRow[];
  /** Text diff only: index in `rows` of the first row of every change block. */
  changeStarts: number[];
}
