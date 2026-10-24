import type { EntrySummary } from '../../models';

/** Opens "Split file…" for the selected files (task 0257). */
export const SPLIT_FILE_ACTION_ID = 'client.splitFile';
/** Opens "Combine files…" for the selected `.001`, `.002`, … parts (task 0257). */
export const COMBINE_FILES_ACTION_ID = 'client.combineFiles';

const KIB = 1024;
const MIB = KIB * 1024;
const GIB = MIB * 1024;

/** Part sizes offered in the split dialog; `custom` lets the user type a size. */
export const SPLIT_PRESETS: readonly { readonly id: string; readonly bytes: number }[] = [
  { id: 'mb10', bytes: 10 * MIB },
  { id: 'mb100', bytes: 100 * MIB },
  { id: 'cd700', bytes: 700 * MIB },
  { id: 'gb1', bytes: GIB },
  { id: 'fat32', bytes: 4 * GIB - 1 },
];

export type SplitUnit = 'KiB' | 'MiB' | 'GiB';

const UNIT_BYTES: Readonly<Record<SplitUnit, number>> = { KiB: KIB, MiB: MIB, GiB: GIB };

/** Bytes for a typed size, or `undefined` when it is not a positive number that fits exactly. */
export function customPartBytes(value: string, unit: SplitUnit): number | undefined {
  const text = value.trim().replace(',', '.');
  if (!/^\d+(\.\d+)?$/.test(text)) return undefined;
  const bytes = Math.floor(Number(text) * UNIT_BYTES[unit]);
  return Number.isSafeInteger(bytes) && bytes >= 1 ? bytes : undefined;
}

/** Number of parts a file of `size` bytes yields; an empty file still yields one empty part. */
export function partCount(size: number, partSize: number): number {
  return size <= 0 ? 1 : Math.ceil(size / partSize);
}

/** Most parts the backend produces for one file. */
export const MAX_SPLIT_PARTS = 10_000;

/** Whether `name` looks like one numbered part (`name.001`, `name.002`, …). */
export function isSplitPartName(name: string): boolean {
  return /^.+\.\d{3,}$/.test(name) && !/\.0+$/.test(name);
}

/** Whether a split makes sense for the entry: a regular, non-empty-named file. */
export function isSplittable(entry: EntrySummary): boolean {
  return entry.kind === 'file';
}

/** Whether the entry is a numbered part of a split file. */
export function isCombinablePart(entry: EntrySummary): boolean {
  return entry.kind === 'file' && isSplitPartName(entry.name);
}
