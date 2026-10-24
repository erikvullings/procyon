import type { EntrySummary } from '../../models';

/** Opens the side-by-side diff of two files (task 0256). */
export const COMPARE_FILES_ACTION_ID = 'client.compareFiles';
/** Compares the two panes' folders byte for byte (task 0256). */
export const COMPARE_BY_CONTENT_ACTION_ID = 'client.compareByContent';

/** The two files to diff, in left/right order. */
export interface FileDiffPair {
  readonly left: EntrySummary;
  readonly right: EntrySummary;
}

/**
 * Resolves what "Compare files" acts on: exactly two files selected in the active pane, or
 * exactly one file selected in each pane (the active pane's file goes on its own side).
 */
export function resolveDiffPair(
  activeSelected: readonly EntrySummary[],
  otherSelected: readonly EntrySummary[],
  activeIsLeft: boolean,
): FileDiffPair | undefined {
  const isFile = (entry: EntrySummary) => entry.kind === 'file';
  const [first, second] = activeSelected;
  if (activeSelected.length === 2 && first !== undefined && second !== undefined) {
    return isFile(first) && isFile(second) ? { left: first, right: second } : undefined;
  }
  const [counterpart] = otherSelected;
  if (
    activeSelected.length === 1 &&
    otherSelected.length === 1 &&
    first !== undefined &&
    counterpart !== undefined &&
    isFile(first) &&
    isFile(counterpart)
  ) {
    return activeIsLeft ? { left: first, right: counterpart } : { left: counterpart, right: first };
  }
  return undefined;
}

/**
 * Row index of the change block after (`1`) or before (`-1`) `current`, wrapping around at the
 * ends; `undefined` when there is no change block.
 */
export function adjacentChange(
  changeStarts: readonly number[],
  current: number,
  direction: 1 | -1,
): number | undefined {
  if (changeStarts.length === 0) return undefined;
  if (direction === 1) {
    return changeStarts.find((start) => start > current) ?? changeStarts[0];
  }
  for (let index = changeStarts.length - 1; index >= 0; index--) {
    const start = changeStarts[index];
    if (start !== undefined && start < current) return start;
  }
  return changeStarts[changeStarts.length - 1];
}

/** Zero-based index of the change block containing or last before `row`; `-1` before the first. */
export function changeBlockAt(changeStarts: readonly number[], row: number): number {
  let found = -1;
  for (let index = 0; index < changeStarts.length; index++) {
    const start = changeStarts[index];
    if (start !== undefined && start <= row) found = index;
  }
  return found;
}
