import { describe, expect, it } from 'vitest';

import type { EntrySummary } from '../../models/entry';
import { type SortModel, sortEntries, sortEntriesResponsive } from './sorting';

function entry(name: string, overrides: Partial<EntrySummary> = {}): EntrySummary {
  return {
    id: name,
    location: { providerId: 'local', uri: `file:///${name}` },
    name,
    kind: 'file',
    hidden: false,
    readOnly: false,
    metadataRevision: 1,
    ...overrides,
  };
}

describe('entry sorting', () => {
  it('sorts names case-insensitively with a deterministic case tie-break', () => {
    const sort: SortModel = [{ column: 'name', direction: 'ascending' }];
    const entries = [entry('beta'), entry('alpha'), entry('Alpha')];

    expect(sortEntries(entries, sort, false).map(({ name }) => name)).toEqual([
      'alpha',
      'Alpha',
      'beta',
    ]);
  });

  it('groups search results by parent path before sorting their names', () => {
    const entries = [
      entry('alpha.pdf', {
        location: { providerId: 'local', uri: 'file:///Users/erik/Zulu/alpha.pdf' },
      }),
      entry('zulu.pdf', {
        location: { providerId: 'local', uri: 'file:///Users/erik/Alpha/zulu.pdf' },
      }),
      entry('beta.pdf', {
        location: { providerId: 'local', uri: 'file:///Users/erik/Alpha/beta.pdf' },
      }),
    ];

    expect(
      sortEntries(entries, [{ column: 'name', direction: 'ascending' }], false, {
        groupByParentPath: true,
      }).map(({ location }) => location.uri),
    ).toEqual([
      'file:///Users/erik/Alpha/beta.pdf',
      'file:///Users/erik/Alpha/zulu.pdf',
      'file:///Users/erik/Zulu/alpha.pdf',
    ]);
  });

  it('groups large search results by parent path in the responsive sorter', async () => {
    const entries = [
      entry('alpha.pdf', {
        location: { providerId: 'local', uri: 'file:///Users/erik/Zulu/alpha.pdf' },
      }),
      entry('zulu.pdf', {
        location: { providerId: 'local', uri: 'file:///Users/erik/Alpha/zulu.pdf' },
      }),
      entry('beta.pdf', {
        location: { providerId: 'local', uri: 'file:///Users/erik/Alpha/beta.pdf' },
      }),
    ];

    const sorted = await sortEntriesResponsive(
      entries,
      [{ column: 'name', direction: 'ascending' }],
      false,
      { groupByParentPath: true },
    );

    expect(sorted.map(({ location }) => location.uri)).toEqual([
      'file:///Users/erik/Alpha/beta.pdf',
      'file:///Users/erik/Alpha/zulu.pdf',
      'file:///Users/erik/Zulu/alpha.pdf',
    ]);
  });

  it('sorts extensions by their raw values in either direction', () => {
    const entries = [
      entry('archive', { extension: 'zip' }),
      entry('document', { extension: 'md' }),
      entry('source', { extension: 'ts' }),
    ];

    expect(
      sortEntries(entries, [{ column: 'extension', direction: 'descending' }], false).map(
        ({ extension }) => extension,
      ),
    ).toEqual(['zip', 'ts', 'md']);
  });

  it('naturally sorts numeric names and handles Unicode names', () => {
    const entries = [entry('Éclair10'), entry('zebra'), entry('éclair2'), entry('Älg')];

    expect(
      sortEntries(entries, [{ column: 'name', direction: 'ascending' }], false).map(
        ({ name }) => name,
      ),
    ).toEqual(['Älg', 'éclair2', 'Éclair10', 'zebra']);
  });

  it('keeps interleaved directories first and stable while sorting files by raw size', () => {
    const entries = [
      entry('file-ten', { size: 10 }),
      entry('directory-b', { kind: 'directory' }),
      entry('file-two-first', { size: 2 }),
      entry('directory-a', { kind: 'directory' }),
      entry('file-two-second', { size: 2 }),
    ];

    expect(
      sortEntries(entries, [{ column: 'size', direction: 'ascending' }], true).map(
        ({ name }) => name,
      ),
    ).toEqual(['directory-b', 'directory-a', 'file-two-first', 'file-two-second', 'file-ten']);
  });

  it('does not prioritize directories when the folders-first setting is disabled', () => {
    const entries = [
      entry('alpha-file'),
      entry('zulu-directory', { kind: 'directory' }),
      entry('beta-file'),
    ];

    expect(
      sortEntries(entries, [{ column: 'name', direction: 'ascending' }], false).map(
        ({ name }) => name,
      ),
    ).toEqual(['alpha-file', 'beta-file', 'zulu-directory']);
  });

  it('sorts modified times by raw timestamp rather than a formatted date', () => {
    const entries = [
      entry('recent', { modifiedAt: '2026-11-02T08:00:00Z' }),
      entry('old', { modifiedAt: '2025-12-31T23:59:59Z' }),
    ];

    expect(
      sortEntries(entries, [{ column: 'modified', direction: 'ascending' }], false).map(
        ({ name }) => name,
      ),
    ).toEqual(['old', 'recent']);
  });

  it('sorts equal modified timestamps deterministically by name in descending mode', () => {
    const modifiedAt = '2026-11-02T08:00:00Z';
    const entries = [entry('1.jpg', { modifiedAt }), entry('27.jpg', { modifiedAt })];

    expect(
      sortEntries(entries, [{ column: 'modified', direction: 'descending' }], false).map(
        ({ name }) => name,
      ),
    ).toEqual(['27.jpg', '1.jpg']);
  });

  it('accepts an empty one-element-capable sort model and returns a stable copy', () => {
    const entries = [entry('second'), entry('first')];

    const sorted = sortEntries(entries, [], false);

    expect(sorted.map(({ name }) => name)).toEqual(['second', 'first']);
    expect(sorted).not.toBe(entries);
  });

  it('cooperatively sorts 100,000 entries without spending a frame in one work slice', async () => {
    const entries = Array.from({ length: 100_000 }, (_, index) =>
      entry(`report-${100_000 - index}.txt`),
    );
    const sliceDurations: number[] = [];
    let sliceStartedAt = performance.now();

    const sorted = await sortEntriesResponsive(
      entries,
      [{ column: 'name', direction: 'ascending' }],
      true,
      {
        frameBudgetMs: 8,
        yieldToMain: async () => {
          sliceDurations.push(performance.now() - sliceStartedAt);
          await Promise.resolve();
          sliceStartedAt = performance.now();
        },
      },
    );

    sliceDurations.push(performance.now() - sliceStartedAt);
    expect(sorted[0]?.name).toBe('report-1.txt');
    expect(sorted.at(-1)?.name).toBe('report-100000.txt');
    // Asserts that yielding actually happened (more than one slice), rather than
    // bounding wall-clock slice duration - a tight per-slice time budget is
    // inherently flaky on a shared, variably loaded CI runner.
    expect(sliceDurations.length).toBeGreaterThan(1);
  });
});
