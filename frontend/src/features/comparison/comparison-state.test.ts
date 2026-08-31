import { describe, expect, it } from 'vitest';

import type { ComparisonEntry, EntrySummary, Location } from '../../models';
import {
  differingEntryIds,
  initialComparisonState,
  relativePathUnder,
  sideForPane,
  statusForEntry,
  withComparisonBatch,
  withComparisonCleared,
  withComparisonError,
  withComparisonStarted,
  withDifferencesOnly,
} from './comparison-state';

const LEFT_ROOT: Location = { providerId: 'local', uri: 'file:///left' };
const RIGHT_ROOT: Location = { providerId: 'local', uri: 'file:///right' };

function entry(overrides: Partial<EntrySummary> = {}): EntrySummary {
  return {
    id: 'entry-1',
    location: { providerId: 'local', uri: 'file:///left/a.txt' },
    name: 'a.txt',
    kind: 'file',
    hidden: false,
    readOnly: false,
    metadataRevision: 1,
    ...overrides,
  };
}

function started() {
  return withComparisonStarted({
    comparisonId: 'comparison-1',
    criteria: 'sizeAndTimestamp',
    leftRoot: LEFT_ROOT,
    rightRoot: RIGHT_ROOT,
    leftPaneId: 'pane-left',
    rightPaneId: 'pane-right',
  });
}

describe('initialComparisonState', () => {
  it('starts with no active comparison and differencesOnly off', () => {
    const state = initialComparisonState();
    expect(state.comparisonId).toBeUndefined();
    expect(state.statusByRelativePath.size).toBe(0);
    expect(state.differencesOnly).toBe(false);
  });
});

describe('withComparisonStarted', () => {
  it('replaces any previous comparison entirely', () => {
    const state = started();
    expect(state.comparisonId).toBe('comparison-1');
    expect(state.leftPaneId).toBe('pane-left');
    expect(state.rightPaneId).toBe('pane-right');
    expect(state.isComplete).toBe(false);
    expect(state.statusByRelativePath.size).toBe(0);
  });
});

describe('withComparisonBatch', () => {
  const entryA: ComparisonEntry = {
    relativePath: 'a.txt',
    status: 'onlyLeft',
    left: { kind: 'file', size: 1 },
  };
  const entryB: ComparisonEntry = { relativePath: 'b.txt', status: 'identical' };
  const entries: ComparisonEntry[] = [entryA, entryB];

  it('merges streamed entries by relative path and updates completion/warnings', () => {
    const next = withComparisonBatch(started(), 'comparison-1', entries, true, 2);
    expect(next.statusByRelativePath.size).toBe(2);
    expect(next.statusByRelativePath.get('a.txt')?.status).toBe('onlyLeft');
    expect(next.isComplete).toBe(true);
    expect(next.warningsCount).toBe(2);
  });

  it('accumulates across multiple batches instead of replacing', () => {
    const first = withComparisonBatch(started(), 'comparison-1', [entryA], false, 0);
    const second = withComparisonBatch(first, 'comparison-1', [entryB], true, 0);
    expect(second.statusByRelativePath.size).toBe(2);
  });

  it('ignores a batch for a comparison that is no longer the tracked one (stale/cancelled)', () => {
    const state = started();
    const next = withComparisonBatch(state, 'some-other-comparison', entries, true, 0);
    expect(next).toBe(state);
  });
});

describe('withComparisonCleared', () => {
  it('resets to the initial state', () => {
    const withData = withComparisonBatch(
      started(),
      'comparison-1',
      [{ relativePath: 'a.txt', status: 'onlyLeft' }],
      true,
      0,
    );
    const cleared = withComparisonCleared();
    expect(cleared.comparisonId).toBeUndefined();
    expect(cleared.statusByRelativePath.size).toBe(0);
    expect(withData.statusByRelativePath.size).toBe(1); // the pre-clear value is untouched
  });
});

describe('withDifferencesOnly / withComparisonError', () => {
  it('toggles the filter flag without touching other fields', () => {
    const next = withDifferencesOnly(started(), true);
    expect(next.differencesOnly).toBe(true);
    expect(next.comparisonId).toBe('comparison-1');
  });

  it('records an error message without touching other fields', () => {
    const next = withComparisonError(started(), 'boom');
    expect(next.error).toBe('boom');
    expect(next.comparisonId).toBe('comparison-1');
  });
});

describe('sideForPane', () => {
  it('identifies the left and right panes and reports neither for an unrelated pane', () => {
    const state = started();
    expect(sideForPane(state, 'pane-left')).toBe('left');
    expect(sideForPane(state, 'pane-right')).toBe('right');
    expect(sideForPane(state, 'pane-other')).toBeUndefined();
  });
});

describe('relativePathUnder', () => {
  it('resolves the root itself to an empty relative path', () => {
    expect(relativePathUnder('file:///left', LEFT_ROOT)).toBe('');
  });

  it('resolves a nested entry to its joined relative path', () => {
    expect(relativePathUnder('file:///left/sub/file.txt', LEFT_ROOT)).toBe('sub/file.txt');
  });

  it('decodes percent-encoded segments', () => {
    expect(relativePathUnder('file:///left/na%C3%AFve.txt', LEFT_ROOT)).toBe('naïve.txt');
  });

  it('returns undefined for an entry outside the root', () => {
    expect(relativePathUnder('file:///elsewhere/file.txt', LEFT_ROOT)).toBeUndefined();
  });

  it('does not treat a same-prefix sibling as nested (file:///left-other must not match file:///left)', () => {
    expect(relativePathUnder('file:///left-other/file.txt', LEFT_ROOT)).toBeUndefined();
  });
});

describe('statusForEntry', () => {
  it('looks up an entry by the left pane, root and relative path', () => {
    const withData = withComparisonBatch(
      started(),
      'comparison-1',
      [{ relativePath: 'sub/file.txt', status: 'newer' }],
      true,
      0,
    );
    const entry = statusForEntry(withData, 'pane-left', 'file:///left/sub/file.txt');
    expect(entry?.status).toBe('newer');
  });

  it('looks up an entry by the right pane using the right root', () => {
    const withData = withComparisonBatch(
      started(),
      'comparison-1',
      [{ relativePath: 'file.txt', status: 'older' }],
      true,
      0,
    );
    const entry = statusForEntry(withData, 'pane-right', 'file:///right/file.txt');
    expect(entry?.status).toBe('older');
  });

  it('returns undefined for a pane not part of the active comparison', () => {
    const withData = withComparisonBatch(
      started(),
      'comparison-1',
      [{ relativePath: 'file.txt', status: 'onlyLeft' }],
      true,
      0,
    );
    expect(statusForEntry(withData, 'pane-unrelated', 'file:///left/file.txt')).toBeUndefined();
  });

  it('returns undefined when no comparison is active', () => {
    expect(
      statusForEntry(initialComparisonState(), 'pane-left', 'file:///left/file.txt'),
    ).toBeUndefined();
  });
});

describe('differingEntryIds', () => {
  it('marks entries whose outcome is not identical, and skips identical/unknown ones (Total-Commander-style selection)', () => {
    const withData = withComparisonBatch(
      started(),
      'comparison-1',
      [
        { relativePath: 'a.txt', status: 'onlyLeft' },
        { relativePath: 'b.txt', status: 'identical' },
      ],
      true,
      0,
    );
    const entries = [
      entry({ id: 'a', location: { providerId: 'local', uri: 'file:///left/a.txt' } }),
      entry({ id: 'b', location: { providerId: 'local', uri: 'file:///left/b.txt' } }),
      entry({ id: 'c', location: { providerId: 'local', uri: 'file:///left/c.txt' } }),
    ];
    expect(differingEntryIds(withData, 'pane-left', entries)).toEqual(['a']);
  });

  it('returns nothing when no comparison is active', () => {
    const entries = [entry({ id: 'a' })];
    expect(differingEntryIds(initialComparisonState(), 'pane-left', entries)).toEqual([]);
  });
});
