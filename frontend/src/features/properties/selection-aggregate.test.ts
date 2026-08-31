import { describe, expect, it } from 'vitest';
import type { EntryId, EntrySummary } from '../../models';
import { computeSelectionAggregate } from './selection-aggregate';

function entry(overrides: Partial<EntrySummary> & { id: string }): EntrySummary {
  return {
    location: { providerId: 'file', uri: `file:///${overrides.id}` },
    name: overrides.id,
    kind: 'file',
    hidden: false,
    readOnly: false,
    metadataRevision: 0,
    ...overrides,
    id: overrides.id as EntryId,
  };
}

describe('computeSelectionAggregate', () => {
  it('returns zeroed totals for an empty selection', () => {
    expect(computeSelectionAggregate([])).toEqual({
      itemCount: 0,
      fileCount: 0,
      folderCount: 0,
      totalSize: 0,
    });
  });

  it('sums sizes and partitions files vs. folders', () => {
    const entries: EntrySummary[] = [
      entry({ id: 'a', kind: 'file', size: 100 }),
      entry({ id: 'b', kind: 'file', size: 250 }),
      entry({ id: 'c', kind: 'directory' }),
      entry({ id: 'd', kind: 'symlink', size: 10 }),
    ];

    expect(computeSelectionAggregate(entries)).toEqual({
      itemCount: 4,
      fileCount: 3,
      folderCount: 1,
      totalSize: 360,
    });
  });

  it('treats a missing size as zero rather than as unknown, so a folder with a known recursive size still contributes', () => {
    const entries: EntrySummary[] = [
      entry({ id: 'a', kind: 'file' }),
      entry({ id: 'b', kind: 'directory', size: 4_096 }),
    ];

    expect(computeSelectionAggregate(entries)).toEqual({
      itemCount: 2,
      fileCount: 1,
      folderCount: 1,
      totalSize: 4_096,
    });
  });
});
