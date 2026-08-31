import type { EntrySummary } from '../../models/entry';

export type SortColumn = 'name' | 'extension' | 'size' | 'modified';
export type SortDirection = 'ascending' | 'descending';

export interface EntrySort {
  column: SortColumn;
  direction: SortDirection;
}

/** The frontend currently supports no sort or one sort key; the tuple leaves room for expansion. */
export type SortModel = readonly [] | readonly [EntrySort];

export interface ResponsiveSortOptions {
  readonly frameBudgetMs?: number;
  readonly yieldToMain?: () => Promise<void>;
  readonly groupByParentPath?: boolean;
}

interface IndexedEntry {
  readonly entry: EntrySummary;
  readonly index: number;
}

const naturalNameCollator = new Intl.Collator('en', {
  numeric: true,
  sensitivity: 'base',
});
const deterministicNameCollator = new Intl.Collator('en', {
  numeric: true,
  sensitivity: 'variant',
  caseFirst: 'lower',
});

function compareNames(left: string, right: string): number {
  return naturalNameCollator.compare(left, right) || deterministicNameCollator.compare(left, right);
}

function compareRaw<T extends number | string>(left: T | undefined, right: T | undefined): number {
  if (left === right) return 0;
  if (left === undefined) return -1;
  if (right === undefined) return 1;
  return left < right ? -1 : 1;
}

function parentPath(entry: EntrySummary): string {
  try {
    const path = decodeURIComponent(new URL(entry.location.uri).pathname).replace(/\/+$/u, '');
    const separator = path.lastIndexOf('/');
    return separator <= 0 ? '/' : path.slice(0, separator);
  } catch {
    return entry.location.uri;
  }
}

function compareByColumn(left: EntrySummary, right: EntrySummary, column: SortColumn): number {
  switch (column) {
    case 'name':
      return compareNames(left.name, right.name);
    case 'extension':
      return compareRaw(left.extension, right.extension);
    case 'size':
      return compareRaw(left.size, right.size);
    case 'modified': {
      const comparison = compareRaw(left.modifiedAt, right.modifiedAt);
      // Archive entries often share the same second-level timestamp; tie-break by name to keep
      // descending/ascending modified sort deterministic and intuitive.
      return comparison !== 0 ? comparison : compareNames(left.name, right.name);
    }
  }
}

/** Returns a sorted copy and preserves input order whenever the configured keys compare equally. */
export function sortEntries(
  entries: readonly EntrySummary[],
  sort: SortModel,
  foldersFirst: boolean,
  options: Pick<ResponsiveSortOptions, 'groupByParentPath'> = {},
): EntrySummary[] {
  const descriptor = sort[0];
  return entries
    .map((entry, index) => ({ entry, index }))
    .sort((left, right) => {
      if (foldersFirst && left.entry.kind !== right.entry.kind) {
        if (left.entry.kind === 'directory') return -1;
        if (right.entry.kind === 'directory') return 1;
      }

      if (descriptor !== undefined) {
        if (descriptor.column === 'name' && options.groupByParentPath === true) {
          const pathComparison = compareNames(parentPath(left.entry), parentPath(right.entry));
          if (pathComparison !== 0) {
            return descriptor.direction === 'ascending' ? pathComparison : -pathComparison;
          }
        }
        const comparison = compareByColumn(left.entry, right.entry, descriptor.column);
        if (comparison !== 0) {
          return descriptor.direction === 'ascending' ? comparison : -comparison;
        }
      }
      return left.index - right.index;
    })
    .map(({ entry }) => entry);
}

function compareIndexed(
  left: IndexedEntry,
  right: IndexedEntry,
  descriptor: EntrySort | undefined,
  foldersFirst: boolean,
  groupByParentPath: boolean,
): number {
  if (foldersFirst && left.entry.kind !== right.entry.kind) {
    if (left.entry.kind === 'directory') return -1;
    if (right.entry.kind === 'directory') return 1;
  }
  if (descriptor !== undefined) {
    if (descriptor.column === 'name' && groupByParentPath) {
      const pathComparison = compareNames(parentPath(left.entry), parentPath(right.entry));
      if (pathComparison !== 0) {
        return descriptor.direction === 'ascending' ? pathComparison : -pathComparison;
      }
    }
    const comparison = compareByColumn(left.entry, right.entry, descriptor.column);
    if (comparison !== 0) {
      return descriptor.direction === 'ascending' ? comparison : -comparison;
    }
  }
  return left.index - right.index;
}

function defaultYieldToMain(): Promise<void> {
  return new Promise((resolve) => {
    if (typeof requestAnimationFrame === 'function') {
      requestAnimationFrame(() => resolve());
    } else {
      setTimeout(resolve, 0);
    }
  });
}

/**
 * Stable cooperative merge sort. Work yields within the configured frame budget,
 * so sorting a large loaded page does not monopolize the browser main thread.
 */
export async function sortEntriesResponsive(
  entries: readonly EntrySummary[],
  sort: SortModel,
  foldersFirst: boolean,
  options: ResponsiveSortOptions = {},
): Promise<EntrySummary[]> {
  const frameBudgetMs = options.frameBudgetMs ?? 8;
  const yieldToMain = options.yieldToMain ?? defaultYieldToMain;
  let sliceStartedAt = performance.now();

  async function checkpoint(): Promise<void> {
    if (performance.now() - sliceStartedAt < frameBudgetMs) {
      return;
    }
    await yieldToMain();
    sliceStartedAt = performance.now();
  }

  let source = new Array<IndexedEntry>(entries.length);
  for (let index = 0; index < entries.length; index += 1) {
    const entry = entries[index];
    if (entry !== undefined) {
      source[index] = { entry, index };
    }
    if ((index & 511) === 0) await checkpoint();
  }

  let target = new Array<IndexedEntry>(entries.length);
  const descriptor = sort[0];
  for (let width = 1; width < entries.length; width *= 2) {
    for (let start = 0; start < entries.length; start += width * 2) {
      const middle = Math.min(start + width, entries.length);
      const end = Math.min(start + width * 2, entries.length);
      let left = start;
      let right = middle;
      for (let output = start; output < end; output += 1) {
        const takeLeft =
          right >= end ||
          (left < middle &&
            compareIndexed(
              source[left] as IndexedEntry,
              source[right] as IndexedEntry,
              descriptor,
              foldersFirst,
              options.groupByParentPath === true,
            ) <= 0);
        target[output] = source[takeLeft ? left++ : right++] as IndexedEntry;
        if ((output & 511) === 0) await checkpoint();
      }
    }
    [source, target] = [target, source];
    await checkpoint();
  }

  const result = new Array<EntrySummary>(entries.length);
  for (let index = 0; index < source.length; index += 1) {
    result[index] = (source[index] as IndexedEntry).entry;
    if ((index & 511) === 0) await checkpoint();
  }
  return result;
}
