import type { EntrySummary, Location } from '../../models';
import { compileQuickFilter, type QuickFilterMatchMode } from './quick-filter';

/** Upper bound on how many sub-directories a single recursive quick-filter walk will list before
 * giving up (task 0310) — keeps an accidental "filter the whole disk" toggle from blocking the UI. */
export const RECURSIVE_FILTER_MAX_DIRECTORIES = 200;
/** Upper bound on how many matched entries to collect (task 0310). */
export const RECURSIVE_FILTER_MAX_MATCHES = 2000;

export interface RecursiveWalkOptions {
  readonly maxDirectories?: number;
  readonly maxMatches?: number;
  /** Task 0314: match mode for plain-text masks (default fuzzy, same as the quick filter). */
  readonly matchMode?: QuickFilterMatchMode;
  /** Aborts a still-running walk; checked between directories. */
  readonly signal?: AbortSignal;
}

/**
 * Bounded breadth-first walk over the sub-directories of a tab's loaded folder, collecting the
 * entries that match a quick-filter query (task 0310). The initial entries are the folder's own
 * already-loaded children; every directory among them (and their descendants, up to the caps) is
 * listed via `listChildren`. Matching uses the real quick-filter compiler, so globs, `;`-separated
 * masks and fuzzy subsequence queries all behave exactly as they do for the current folder — the
 * walk only *widens the set of entries* the same matcher is applied to.
 *
 * Unreadable directories are skipped (the walk continues); a blank query matches every entry it
 * reaches (still bounded by the caps).
 */
export async function collectRecursiveQuickFilterEntries(
  initialEntries: readonly EntrySummary[],
  query: string,
  listChildren: (location: Location, signal: AbortSignal) => Promise<readonly EntrySummary[]>,
  options: RecursiveWalkOptions = {},
): Promise<readonly EntrySummary[]> {
  const {
    maxDirectories = RECURSIVE_FILTER_MAX_DIRECTORIES,
    maxMatches = RECURSIVE_FILTER_MAX_MATCHES,
    matchMode,
    signal,
  } = options;
  const matcher = compileQuickFilter(query, matchMode ?? 'fuzzy') ?? (() => true);
  const walkSignal = signal ?? new AbortController().signal;
  const matches: EntrySummary[] = [];
  const seenDirectories = new Set<string>();
  const queue: Location[] = [];
  let directoriesVisited = 0;

  const consider = (entries: readonly EntrySummary[]): void => {
    for (const entry of entries) {
      if (matches.length >= maxMatches) return;
      if (matcher(entry.name)) matches.push(entry);
      if (entry.kind === 'directory' && queue.length < maxDirectories) {
        queue.push(entry.location);
      }
    }
  };

  consider(initialEntries);
  while (queue.length > 0) {
    if (signal?.aborted) return matches;
    if (directoriesVisited >= maxDirectories || matches.length >= maxMatches) break;
    const directory = queue.shift();
    if (directory === undefined || seenDirectories.has(directory.uri)) continue;
    seenDirectories.add(directory.uri);
    directoriesVisited += 1;
    let children: readonly EntrySummary[];
    try {
      children = await listChildren(directory, walkSignal);
    } catch {
      continue; // skip an unreadable directory but keep walking the rest
    }
    consider(children);
  }
  return matches;
}
