import type { EntryId, EntrySummary } from '../../models';

/**
 * How one `;`-separated quick-filter mask matches (spec §24). The mode is auto-detected per mask:
 * a mask containing `*` or `?` is a case-insensitive glob anchored to the whole name, anything
 * else is a case-insensitive substring. Regex matching is not implemented.
 */
export type QuickFilterMode = 'plainText' | 'glob';

/**
 * Marks a text input whose Cmd/Ctrl+C and Cmd/Ctrl+X, pressed with no text selected, should reach
 * the global file-clipboard shortcut instead of being treated as text editing.
 */
export const FILE_CLIPBOARD_PASSTHROUGH_ATTRIBUTE = 'data-file-clipboard-passthrough';

/** Separates alternative quick-filter masks; an entry matching any mask is shown. */
export const QUICK_FILTER_MASK_SEPARATOR = ';';

/** Detects whether a single mask is a glob (`*`/`?`) or a plain-text substring. */
export function quickFilterMode(mask: string): QuickFilterMode {
  return /[*?]/u.test(mask) ? 'glob' : 'plainText';
}

function globMaskRegExp(pattern: string): RegExp {
  if (pattern === '*.*') return /^/u;
  let source = '^';
  for (const character of pattern) {
    if (character === '*') source += '.*';
    else if (character === '?') source += '.';
    else source += character.replace(/[\\^$.*+?()[\]{}|]/gu, '\\$&');
  }
  return new RegExp(`${source}$`, 'iu');
}

/** Matches a filename against a case-insensitive `*`/`?` glob mask. */
export function matchesGlobMask(name: string, pattern: string): boolean {
  return globMaskRegExp(pattern).test(name);
}

/**
 * Compiles a quick-filter query into a name predicate, or `undefined` when the query has no
 * non-blank masks. Masks are split on `;`, trimmed, and OR-ed together.
 */
export function compileQuickFilter(query: string): ((name: string) => boolean) | undefined {
  const matchers = query
    .split(QUICK_FILTER_MASK_SEPARATOR)
    .map((mask) => mask.trim())
    .filter((mask) => mask !== '')
    .map((mask): ((name: string) => boolean) => {
      if (quickFilterMode(mask) === 'glob') {
        const pattern = globMaskRegExp(mask);
        return (name) => pattern.test(name);
      }
      const needle = mask.toLocaleLowerCase();
      return (name) => name.toLocaleLowerCase().includes(needle);
    });
  if (matchers.length === 0) return undefined;
  if (matchers.length === 1) return matchers[0];
  return (name) => matchers.some((matches) => matches(name));
}

/** True when the entry's display name matches the quick-filter query (blank matches everything). */
export function matchesQuickFilter(entry: EntrySummary, query: string): boolean {
  return compileQuickFilter(query)?.(entry.name) ?? true;
}

/** Filters entries by a quick-filter query; a blank query returns the input reference unchanged. */
export function filterEntries(
  entries: readonly EntrySummary[],
  query: string,
): readonly EntrySummary[] {
  const matches = compileQuickFilter(query);
  if (matches === undefined) return entries;
  return entries.filter((entry) => matches(entry.name));
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
