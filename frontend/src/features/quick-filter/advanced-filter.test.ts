import { describe, expect, it } from 'vitest';

import type { EntryId, EntrySummary } from '../../models';
import {
  type AdvancedFilterCriteria,
  advancedFilterNotices,
  EMPTY_ADVANCED_FILTER,
  filterEntriesAdvanced,
  formatFilterSize,
  isAdvancedFilterActive,
  isExecutableEntry,
  parseFilterSize,
} from './advanced-filter';

const NOW = Date.parse('2026-10-24T12:00:00Z');
const daysAgo = (days: number): string => new Date(NOW - days * 86_400_000).toISOString();

function entry(name: string, overrides: Partial<EntrySummary> = {}): EntrySummary {
  return {
    id: name as EntryId,
    location: { providerId: 'file', uri: `file:///d/${name}` },
    name,
    kind: 'file',
    hidden: false,
    readOnly: false,
    metadataRevision: 1,
    ...overrides,
  };
}

// Deliberately unsorted and interleaved across kinds, sizes, ages and hidden state.
const listing: readonly EntrySummary[] = [
  entry('zeta.mkv', { size: 5_000_000, modifiedAt: daysAgo(2) }),
  entry('docs', { kind: 'directory', modifiedAt: daysAgo(1) }),
  entry('.env', { size: 20, hidden: true, modifiedAt: daysAgo(400) }),
  entry('alpha.mp4', { size: 900, modifiedAt: daysAgo(40) }),
  entry('run.sh', { size: 300, modifiedAt: daysAgo(5) }),
  entry('link', { kind: 'symlink', modifiedAt: daysAgo(9) }),
  entry('app.EXE', { size: 8_000_000, modifiedAt: daysAgo(100) }),
  entry('mystery.bin'),
];

function names(criteria: Partial<AdvancedFilterCriteria>): string[] {
  return filterEntriesAdvanced(listing, { ...EMPTY_ADVANCED_FILTER, ...criteria }, NOW).map(
    (e) => e.name,
  );
}

describe('filterEntriesAdvanced', () => {
  it('returns the input reference when no criterion is set', () => {
    expect(filterEntriesAdvanced(listing, EMPTY_ADVANCED_FILTER, NOW)).toBe(listing);
    expect(isAdvancedFilterActive(EMPTY_ADVANCED_FILTER)).toBe(false);
  });

  it('matches name masks with globs and alternatives', () => {
    expect(names({ name: '*.mkv;*.mp4' })).toEqual(['zeta.mkv', 'alpha.mp4']);
    expect(names({ name: 'RUN' })).toEqual(['run.sh']);
  });

  it('filters by kind', () => {
    expect(names({ kind: 'directory' })).toEqual(['docs']);
    expect(names({ kind: 'symlink' })).toEqual(['link']);
    expect(names({ kind: 'file' })).toHaveLength(6);
  });

  it('applies size bounds to files with a known size only', () => {
    expect(names({ minSize: 1000 })).toEqual(['zeta.mkv', 'app.EXE']);
    expect(names({ maxSize: 900 })).toEqual(['.env', 'alpha.mp4', 'run.sh']);
    expect(names({ minSize: 300, maxSize: 900 })).toEqual(['alpha.mp4', 'run.sh']);
  });

  it('applies modified age within and older-than bounds', () => {
    expect(names({ modifiedWithinDays: 7 })).toEqual(['zeta.mkv', 'docs', 'run.sh']);
    expect(names({ modifiedOlderThanDays: 90 })).toEqual(['.env', 'app.EXE']);
    expect(names({ modifiedWithinDays: 60, modifiedOlderThanDays: 10 })).toEqual(['alpha.mp4']);
  });

  it('handles hidden and executable tri-states', () => {
    expect(names({ hidden: 'only' })).toEqual(['.env']);
    expect(names({ hidden: 'exclude', kind: 'file', name: 'e' })).toEqual([
      'zeta.mkv',
      'app.EXE',
      'mystery.bin',
    ]);
    expect(names({ executable: 'only' })).toEqual(['run.sh', 'app.EXE']);
    expect(names({ executable: 'exclude', kind: 'file' })).toEqual([
      'zeta.mkv',
      '.env',
      'alpha.mp4',
      'mystery.bin',
    ]);
  });

  it('combines every criterion with AND', () => {
    expect(
      names({
        name: '*.sh;*.exe',
        kind: 'file',
        minSize: 100,
        modifiedWithinDays: 30,
        hidden: 'exclude',
        executable: 'only',
      }),
    ).toEqual(['run.sh']);
  });

  it('never treats directories as executable', () => {
    expect(isExecutableEntry(entry('bin.sh', { kind: 'directory' }))).toBe(false);
    expect(isExecutableEntry(entry('Makefile'))).toBe(false);
  });
});

describe('advancedFilterNotices', () => {
  it('is empty for inactive criteria', () => {
    expect(advancedFilterNotices(listing, EMPTY_ADVANCED_FILTER, { hasMore: true })).toEqual([]);
  });

  it('explains provider metadata gaps instead of ignoring criteria', () => {
    const notices = advancedFilterNotices(
      listing,
      { ...EMPTY_ADVANCED_FILTER, minSize: 1, modifiedWithinDays: 3, executable: 'only' },
      { hasMore: false },
    );
    expect(notices).toEqual(['sizeUnavailable', 'modifiedUnavailable', 'executableByExtension']);
  });

  it('does not warn about sizes when every file reports one', () => {
    const sized = [entry('a', { size: 1, modifiedAt: daysAgo(1) }), listing[1] as EntrySummary];
    expect(
      advancedFilterNotices(sized, { ...EMPTY_ADVANCED_FILTER, minSize: 1 }, { hasMore: false }),
    ).toEqual([]);
  });

  it('warns when hidden-only finds no hidden entries and when paging leaves pages unloaded', () => {
    const visible = [entry('a')];
    expect(
      advancedFilterNotices(
        visible,
        { ...EMPTY_ADVANCED_FILTER, hidden: 'only' },
        { hasMore: true },
      ),
    ).toEqual(['noHiddenLoaded', 'loadedEntriesOnly']);
  });
});

describe('filter sizes', () => {
  it('parses binary units and rejects junk', () => {
    expect(parseFilterSize('10')).toBe(10);
    expect(parseFilterSize('1.5 MB')).toBe(1_572_864);
    expect(parseFilterSize('2gib')).toBe(2 * 1024 ** 3);
    expect(parseFilterSize('abc')).toBeUndefined();
    expect(parseFilterSize('-1')).toBeUndefined();
    expect(parseFilterSize('5 parsecs')).toBeUndefined();
  });

  it('round-trips through the formatter', () => {
    for (const bytes of [0, 7, 1024, 1_572_864, 3 * 1024 ** 3]) {
      expect(parseFilterSize(formatFilterSize(bytes))).toBe(bytes);
    }
  });
});
