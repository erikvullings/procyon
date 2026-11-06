import type { EntryId, EntrySummary } from '../../models';
import { DEFAULT_QUICK_FILTER_MATCH_MODE, type QuickFilterMatchMode } from '../../models/settings';
import { fuzzyMatches, substringMatches } from '../../utilities/fuzzy-match';

/**
 * How one `;`-separated quick-filter mask matches (spec §24). The mode is auto-detected per mask:
 * a mask containing `*` or `?` is a case-insensitive glob anchored to the whole name, anything
 * else is matched per the active {@link QuickFilterMatchMode} (fuzzy subsequence by default, or a
 * strict substring in `contains` mode — task 0314). Regex matching is not implemented.
 */
export type QuickFilterMode = 'plainText' | 'glob';

/** Task 0314: fuzzy (0310 default, forgiving) vs contains (old substring, exact). The type and
 * default are declared once in `models/settings.ts` (where the persisted `Settings` lives) and
 * re-exported here for the quick-filter callers. */
export type { QuickFilterMatchMode } from '../../models/settings';
export { DEFAULT_QUICK_FILTER_MATCH_MODE } from '../../models/settings';

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
 * non-blank masks. Masks are split on `;`, trimmed, and OR-ed together. `mode` (task 0314)
 * selects fuzzy-subsequence vs strict-substring for plain-text masks; it defaults to `fuzzy` so
 * existing callers keep the 0310 behaviour unless they explicitly pass the global mode.
 */
export function compileQuickFilter(
  query: string,
  mode: QuickFilterMatchMode = DEFAULT_QUICK_FILTER_MATCH_MODE,
): ((name: string) => boolean) | undefined {
  const matchMode = mode === 'contains' ? substringMatches : fuzzyMatches;
  const matchers = query
    .split(QUICK_FILTER_MASK_SEPARATOR)
    .map((mask) => mask.trim())
    .filter((mask) => mask !== '')
    .map((mask): ((name: string) => boolean) => {
      if (quickFilterMode(mask) === 'glob') {
        const pattern = globMaskRegExp(mask);
        return (name) => pattern.test(name);
      }
      return (name) => matchMode(mask, name);
    });
  if (matchers.length === 0) return undefined;
  if (matchers.length === 1) return matchers[0];
  return (name) => matchers.some((matches) => matches(name));
}

/** True when the entry's display name matches the quick-filter query (blank matches everything),
 * under match `mode` (task 0314, default fuzzy). */
export function matchesQuickFilter(
  entry: EntrySummary,
  query: string,
  mode: QuickFilterMatchMode = DEFAULT_QUICK_FILTER_MATCH_MODE,
): boolean {
  return compileQuickFilter(query, mode)?.(entry.name) ?? true;
}

/** Filters entries by a quick-filter query; a blank query returns the input reference unchanged.
 * `mode` (task 0314) is threaded to {@link compileQuickFilter} and defaults to fuzzy. */
export function filterEntries(
  entries: readonly EntrySummary[],
  query: string,
  mode: QuickFilterMatchMode = DEFAULT_QUICK_FILTER_MATCH_MODE,
): readonly EntrySummary[] {
  const matches = compileQuickFilter(query, mode);
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
