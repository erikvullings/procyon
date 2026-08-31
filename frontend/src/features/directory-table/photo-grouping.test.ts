import { describe, expect, it } from 'vitest';
import type { EntrySummary } from '../../models';
import { groupEntriesByDay, layoutPhotoLines } from './photo-grouping';

function entry(name: string, modifiedAt: string): EntrySummary {
  return {
    id: name,
    location: { providerId: 'file', uri: `file:///tmp/${name}` },
    name,
    kind: 'file',
    size: 1,
    modifiedAt,
    hidden: false,
    readOnly: false,
    extension: 'png',
    metadataRevision: 1,
  };
}

describe('groupEntriesByDay', () => {
  it('groups contiguous entries sharing the same calendar day', () => {
    const entries = [
      entry('a.png', '2026-08-04T10:00:00.000Z'),
      entry('b.png', '2026-08-04T18:00:00.000Z'),
      entry('c.png', '2026-08-03T09:00:00.000Z'),
    ];
    const groups = groupEntriesByDay(entries);
    expect(groups).toEqual([
      { dayKey: '2026-08-04', label: expect.any(String), startIndex: 0, count: 2 },
      { dayKey: '2026-08-03', label: expect.any(String), startIndex: 2, count: 1 },
    ]);
  });

  it('starts a new group when the same day reappears non-contiguously', () => {
    const entries = [
      entry('a.png', '2026-08-04T10:00:00.000Z'),
      entry('b.png', '2026-08-03T09:00:00.000Z'),
      entry('c.png', '2026-08-04T11:00:00.000Z'),
    ];
    const groups = groupEntriesByDay(entries);
    expect(groups.map((g) => [g.dayKey, g.startIndex, g.count])).toEqual([
      ['2026-08-04', 0, 1],
      ['2026-08-03', 1, 1],
      ['2026-08-04', 2, 1],
    ]);
  });

  it('buckets entries with missing/invalid modifiedAt under "unknown"', () => {
    const entries = [entry('a.png', ''), entry('b.png', 'not-a-date')];
    const groups = groupEntriesByDay(entries);
    expect(groups).toHaveLength(1);
    expect(groups[0]?.dayKey).toBe('unknown');
    expect(groups[0]?.count).toBe(2);
  });

  it('returns an empty array for no entries', () => {
    expect(groupEntriesByDay([])).toEqual([]);
  });
});

describe('layoutPhotoLines', () => {
  it('emits one header line per group and wraps tiles into rows of columnsPerRow', () => {
    const entries = [
      entry('a.png', '2026-08-04T10:00:00.000Z'),
      entry('b.png', '2026-08-04T18:00:00.000Z'),
      entry('c.png', '2026-08-04T12:00:00.000Z'),
      entry('d.png', '2026-08-03T09:00:00.000Z'),
    ];
    const lines = layoutPhotoLines(entries, 2);
    expect(lines).toEqual([
      { kind: 'header', label: expect.any(String) },
      { kind: 'row', startIndex: 0, count: 2 },
      { kind: 'row', startIndex: 2, count: 1 },
      { kind: 'header', label: expect.any(String) },
      { kind: 'row', startIndex: 3, count: 1 },
    ]);
  });

  it('handles a single column (columnsPerRow=1)', () => {
    const entries = [entry('a.png', '2026-08-04T10:00:00.000Z')];
    const lines = layoutPhotoLines(entries, 1);
    expect(lines).toEqual([
      { kind: 'header', label: expect.any(String) },
      { kind: 'row', startIndex: 0, count: 1 },
    ]);
  });

  it('returns an empty array for no entries', () => {
    expect(layoutPhotoLines([], 4)).toEqual([]);
  });
});
