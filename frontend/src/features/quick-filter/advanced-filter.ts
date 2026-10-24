import type { EntrySummary } from '../../models';
import type { FilterCriteria, FilterToggle } from '../../models/settings';
import { compileQuickFilter } from './quick-filter';

export type AdvancedFilterCriteria = FilterCriteria;

/** Criteria that match every entry. */
export const EMPTY_ADVANCED_FILTER: AdvancedFilterCriteria = {
  name: '',
  kind: 'any',
  minSize: null,
  maxSize: null,
  modifiedWithinDays: null,
  modifiedOlderThanDays: null,
  hidden: 'any',
  executable: 'any',
};

const MS_PER_DAY = 86_400_000;

/**
 * Listings carry no permission bits, so "executable" is a file with an extension that launches
 * programs on the common platforms. The panel says so (`executableByExtension`).
 */
const EXECUTABLE_EXTENSIONS: ReadonlySet<string> = new Set([
  'exe',
  'bat',
  'cmd',
  'com',
  'msi',
  'ps1',
  'sh',
  'bash',
  'zsh',
  'command',
  'appimage',
  'run',
]);

/** Which criteria are set, in display order. */
export type ActiveCriterionId =
  | 'name'
  | 'kind'
  | 'minSize'
  | 'maxSize'
  | 'modifiedWithinDays'
  | 'modifiedOlderThanDays'
  | 'hidden'
  | 'executable';

export function activeCriteriaIds(criteria: AdvancedFilterCriteria): readonly ActiveCriterionId[] {
  const ids: ActiveCriterionId[] = [];
  if (criteria.name.trim() !== '') ids.push('name');
  if (criteria.kind !== 'any') ids.push('kind');
  if (criteria.minSize !== null) ids.push('minSize');
  if (criteria.maxSize !== null) ids.push('maxSize');
  if (criteria.modifiedWithinDays !== null) ids.push('modifiedWithinDays');
  if (criteria.modifiedOlderThanDays !== null) ids.push('modifiedOlderThanDays');
  if (criteria.hidden !== 'any') ids.push('hidden');
  if (criteria.executable !== 'any') ids.push('executable');
  return ids;
}

export function isAdvancedFilterActive(criteria: AdvancedFilterCriteria): boolean {
  return activeCriteriaIds(criteria).length > 0;
}

export function advancedFilterEquals(
  left: AdvancedFilterCriteria,
  right: AdvancedFilterCriteria,
): boolean {
  return (
    left.name === right.name &&
    left.kind === right.kind &&
    left.minSize === right.minSize &&
    left.maxSize === right.maxSize &&
    left.modifiedWithinDays === right.modifiedWithinDays &&
    left.modifiedOlderThanDays === right.modifiedOlderThanDays &&
    left.hidden === right.hidden &&
    left.executable === right.executable
  );
}

function extensionOf(entry: EntrySummary): string {
  if (entry.extension !== undefined) return entry.extension.replace(/^\./u, '').toLowerCase();
  const dot = entry.name.lastIndexOf('.');
  return dot <= 0 ? '' : entry.name.slice(dot + 1).toLowerCase();
}

/** Whether the entry would be treated as executable (see {@link EXECUTABLE_EXTENSIONS}). */
export function isExecutableEntry(entry: EntrySummary): boolean {
  return entry.kind === 'file' && EXECUTABLE_EXTENSIONS.has(extensionOf(entry));
}

function togglePasses(toggle: FilterToggle, value: boolean): boolean {
  return toggle === 'any' || (toggle === 'only' ? value : !value);
}

/** Compiles criteria into one predicate; `now` is epoch milliseconds for the age criteria. */
export function compileAdvancedFilter(
  criteria: AdvancedFilterCriteria,
  now: number,
): ((entry: EntrySummary) => boolean) | undefined {
  if (!isAdvancedFilterActive(criteria)) return undefined;
  const nameMatches = compileQuickFilter(criteria.name);
  const newest =
    criteria.modifiedWithinDays === null
      ? undefined
      : now - criteria.modifiedWithinDays * MS_PER_DAY;
  const oldest =
    criteria.modifiedOlderThanDays === null
      ? undefined
      : now - criteria.modifiedOlderThanDays * MS_PER_DAY;
  return (entry) => {
    if (nameMatches !== undefined && !nameMatches(entry.name)) return false;
    if (criteria.kind !== 'any' && entry.kind !== criteria.kind) return false;
    if (criteria.minSize !== null || criteria.maxSize !== null) {
      // Directories and entries whose provider reports no size cannot satisfy a size bound.
      if (entry.kind === 'directory' || entry.size === undefined) return false;
      if (criteria.minSize !== null && entry.size < criteria.minSize) return false;
      if (criteria.maxSize !== null && entry.size > criteria.maxSize) return false;
    }
    if (newest !== undefined || oldest !== undefined) {
      const modified = entry.modifiedAt === undefined ? Number.NaN : Date.parse(entry.modifiedAt);
      if (Number.isNaN(modified)) return false;
      if (newest !== undefined && modified < newest) return false;
      if (oldest !== undefined && modified >= oldest) return false;
    }
    if (!togglePasses(criteria.hidden, entry.hidden)) return false;
    return togglePasses(criteria.executable, isExecutableEntry(entry));
  };
}

/** Filters loaded entries; inactive criteria return the input reference unchanged. */
export function filterEntriesAdvanced(
  entries: readonly EntrySummary[],
  criteria: AdvancedFilterCriteria,
  now: number = Date.now(),
): readonly EntrySummary[] {
  const matches = compileAdvancedFilter(criteria, now);
  return matches === undefined ? entries : entries.filter(matches);
}

/** Why a criterion may not behave as the user expects on the current listing. */
export type AdvancedFilterNotice =
  | 'sizeUnavailable'
  | 'modifiedUnavailable'
  | 'noHiddenLoaded'
  | 'executableByExtension'
  | 'loadedEntriesOnly';

/**
 * Explains criteria the listing cannot fully honour, rather than silently ignoring them.
 * `hasMore` means unloaded pages exist, which the filter never fetches.
 */
export function advancedFilterNotices(
  entries: readonly EntrySummary[],
  criteria: AdvancedFilterCriteria,
  options: { readonly hasMore: boolean },
): readonly AdvancedFilterNotice[] {
  if (!isAdvancedFilterActive(criteria)) return [];
  const notices: AdvancedFilterNotice[] = [];
  const sizeBound = criteria.minSize !== null || criteria.maxSize !== null;
  if (sizeBound && entries.some((e) => e.kind !== 'directory' && e.size === undefined)) {
    notices.push('sizeUnavailable');
  }
  const ageBound = criteria.modifiedWithinDays !== null || criteria.modifiedOlderThanDays !== null;
  if (ageBound && entries.some((e) => e.modifiedAt === undefined)) {
    notices.push('modifiedUnavailable');
  }
  if (criteria.hidden === 'only' && !entries.some((e) => e.hidden)) {
    notices.push('noHiddenLoaded');
  }
  if (criteria.executable !== 'any') notices.push('executableByExtension');
  if (options.hasMore) notices.push('loadedEntriesOnly');
  return notices;
}

const SIZE_UNITS: ReadonlyArray<readonly [string, number]> = [
  ['tb', 1024 ** 4],
  ['gb', 1024 ** 3],
  ['mb', 1024 ** 2],
  ['kb', 1024],
  ['b', 1],
];

/** Parses "10", "1.5 MB", "2gb" (binary units) to bytes; `undefined` when invalid. */
export function parseFilterSize(text: string): number | undefined {
  const match = /^\s*(\d+(?:\.\d+)?)\s*([a-z]*)\s*$/iu.exec(text);
  if (match === null) return undefined;
  const unit = (match[2] ?? '').toLowerCase().replace(/ib$/u, 'b');
  const factor = unit === '' ? 1 : SIZE_UNITS.find(([name]) => name === unit)?.[1];
  if (factor === undefined) return undefined;
  const bytes = Math.round(Number(match[1]) * factor);
  return Number.isSafeInteger(bytes) ? bytes : undefined;
}

/** Inverse of {@link parseFilterSize}, choosing the largest exact unit. */
export function formatFilterSize(bytes: number): string {
  for (const [name, factor] of SIZE_UNITS) {
    if (bytes >= factor && bytes % factor === 0) return `${bytes / factor} ${name.toUpperCase()}`;
  }
  return `${bytes} B`;
}
