import type { EntryId, EntrySummary } from '../../models';

/**
 * Only plain-text matching is implemented today. Glob/regex modes are designed for (spec §24)
 * but intentionally not built — this single-variant union is the placeholder acceptance criteria
 * calls for.
 */
export type QuickFilterMode = 'plainText';

/** Matches a filename against a case-insensitive `*`/`?` glob mask. */
export function matchesGlobMask(name: string, pattern: string): boolean {
  if (pattern === '*.*') return true;
  let source = '^';
  for (const character of pattern) {
    if (character === '*') source += '.*';
    else if (character === '?') source += '.';
    else source += character.replace(/[\\^$.*+?()[\]{}|]/gu, '\\$&');
  }
  return new RegExp(`${source}$`, 'iu').test(name);
}

/** Case-insensitive plain-text match against an entry's display name. */
export function matchesQuickFilter(entry: EntrySummary, query: string): boolean {
  return entry.name.toLocaleLowerCase().includes(query.toLocaleLowerCase());
}

/** Filters entries by a plain-text query; a blank query returns the input reference unchanged. */
export function filterEntries(
  entries: readonly EntrySummary[],
  query: string,
): readonly EntrySummary[] {
  const trimmed = query.trim();
  if (trimmed === '') return entries;
  return entries.filter((entry) => matchesQuickFilter(entry, trimmed));
}

/** Counts selected entries present in `entries` but absent from the filtered `visibleEntries`. */
export function hiddenSelectedEntryCount(
  entries: readonly EntrySummary[],
  visibleEntries: readonly EntrySummary[],
  selectedEntryIds: ReadonlySet<EntryId>,
): number {
  const visibleIds = new Set(visibleEntries.map((entry) => entry.id));
  let count = 0;
  for (const entry of entries) {
    if (selectedEntryIds.has(entry.id) && !visibleIds.has(entry.id)) {
      count += 1;
    }
  }
  return count;
}
