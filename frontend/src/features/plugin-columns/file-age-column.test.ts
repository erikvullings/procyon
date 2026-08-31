import { describe, expect, it } from 'vitest';

import { fileAgeColumn } from './file-age-column';

describe('fileAgeColumn', () => {
  const now = Date.UTC(2026, 7, 1, 12, 0, 0);

  it.each([
    [59, '0m'],
    [60, '1m'],
    [23 * 60 * 60, '23h'],
    [24 * 60 * 60, '1d'],
    [30 * 24 * 60 * 60, '1mo'],
    [12 * 30 * 24 * 60 * 60, '1y'],
  ])('formats an age of %is as %s at the documented boundaries', (seconds, expected) => {
    expect(fileAgeColumn.display(new Date(now - seconds * 1_000).toISOString(), now)).toBe(
      expected,
    );
  });

  it('sorts interleaved entries by their raw timestamps rather than display text', () => {
    const entries = [
      { id: 'old', modifiedAt: new Date(now - 90 * 24 * 60 * 60 * 1_000).toISOString() },
      { id: 'recent', modifiedAt: new Date(now - 5 * 60 * 1_000).toISOString() },
      { id: 'middle', modifiedAt: new Date(now - 3 * 60 * 60 * 1_000).toISOString() },
    ];

    expect([...entries].sort(fileAgeColumn.compare).map((entry) => entry.id)).toEqual([
      'old',
      'middle',
      'recent',
    ]);
  });

  it('only reports entries whose compact value changed on a minute refresh', () => {
    expect(
      fileAgeColumn.changedEntryIds(
        [
          { id: 'stable' },
          { id: 'boundary', modifiedAt: new Date(now - 59 * 1_000).toISOString() },
        ],
        now,
        now + 60 * 1_000,
      ),
    ).toEqual(['boundary']);
  });
});
