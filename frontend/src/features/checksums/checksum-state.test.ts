import { describe, expect, it } from 'vitest';
import type { ChecksumEntry, DuplicateGroup, Location } from '../../models';
import {
  initialChecksumState,
  initialDuplicateState,
  selectedLocations,
  totalReclaimableBytes,
  withChecksumBatch,
  withChecksumCleared,
  withChecksumError,
  withChecksumJobStarted,
  withDuplicateResults,
  withDuplicateScanStarted,
  withDuplicateSelectionCleared,
  withDuplicateSelectionToggled,
  withVerificationReport,
  wouldDeleteEveryCopy,
} from './checksum-state';

function location(uri: string): Location {
  return { providerId: 'local', uri };
}

function entry(relativePath: string, digest: string): ChecksumEntry {
  return {
    location: location(`file:///root/${relativePath}`),
    relativePath,
    size: 3,
    checksums: { sha256: digest },
  };
}

function group(overrides: Partial<DuplicateGroup> = {}): DuplicateGroup {
  return {
    fullHash: 'abc',
    size: 100,
    hardlinkClusters: [],
    distinctLocations: [location('file:///root/a'), location('file:///root/b')],
    reclaimableBytes: 100,
    ...overrides,
  };
}

describe('checksum state', () => {
  it('starts empty', () => {
    const state = initialChecksumState();
    expect(state.jobId).toBeUndefined();
    expect(state.entries).toEqual([]);
    expect(state.isComplete).toBe(false);
  });

  it('appends streamed batches rather than replacing them', () => {
    let state = withChecksumJobStarted('job-1', ['sha256'], 2);
    state = withChecksumBatch(state, 'job-1', [entry('a.txt', 'aa')], false, false);
    state = withChecksumBatch(state, 'job-1', [entry('b.txt', 'bb')], true, false);
    expect(state.entries.map((item) => item.relativePath)).toEqual(['a.txt', 'b.txt']);
    expect(state.isComplete).toBe(true);
    expect(state.totalEntries).toBe(2);
  });

  it('ignores a batch for a different job', () => {
    const state = withChecksumJobStarted('job-1', ['sha256'], 1);
    const next = withChecksumBatch(state, 'job-2', [entry('a.txt', 'aa')], true, false);
    expect(next).toBe(state);
  });

  it('records cancellation distinctly from completion', () => {
    let state = withChecksumJobStarted('job-1', ['sha256'], 5);
    state = withChecksumBatch(state, 'job-1', [entry('a.txt', 'aa')], true, true);
    expect(state.isComplete).toBe(true);
    expect(state.isCancelled).toBe(true);
    expect(state.entries).toHaveLength(1);
    expect(state.totalEntries).toBe(5);
  });

  it('ignores a verification report for a different job', () => {
    const state = withChecksumJobStarted('job-1', ['sha256'], 1);
    const next = withVerificationReport(state, {
      jobId: 'other',
      results: [],
      matched: 0,
      mismatched: 0,
      missing: 0,
    });
    expect(next).toBe(state);
  });

  it('keeps a verification report for the tracked job', () => {
    const state = withChecksumJobStarted('job-1', ['sha256'], 1);
    const next = withVerificationReport(state, {
      jobId: 'job-1',
      results: [{ path: 'a.txt', status: 'match' }],
      matched: 1,
      mismatched: 0,
      missing: 0,
    });
    expect(next.verification?.matched).toBe(1);
  });

  it('clears back to the initial state', () => {
    expect(withChecksumCleared()).toEqual(initialChecksumState());
  });

  it('records an error without losing the job', () => {
    const state = withChecksumError(withChecksumJobStarted('job-1', ['sha256'], 1), 'nope');
    expect(state.error).toBe('nope');
    expect(state.jobId).toBe('job-1');
  });
});

describe('duplicate state', () => {
  it('applies results only for the tracked scan', () => {
    const state = withDuplicateScanStarted('scan-1', [location('file:///root')]);
    expect(withDuplicateResults(state, 'other', [group()], false, 0)).toBe(state);

    const next = withDuplicateResults(state, 'scan-1', [group()], false, 0);
    expect(next.groups).toHaveLength(1);
    expect(next.isComplete).toBe(true);
  });

  it('flags a cancelled scan and carries no groups', () => {
    const state = withDuplicateScanStarted('scan-1', [location('file:///root')]);
    const next = withDuplicateResults(state, 'scan-1', [], true, 0);
    expect(next.isCancelled).toBe(true);
    expect(next.groups).toEqual([]);
  });

  it('toggles a path on and off', () => {
    let state = withDuplicateResults(
      withDuplicateScanStarted('scan-1', []),
      'scan-1',
      [group()],
      false,
      0,
    );
    state = withDuplicateSelectionToggled(state, 'file:///root/a');
    expect(state.selectedUris.has('file:///root/a')).toBe(true);
    state = withDuplicateSelectionToggled(state, 'file:///root/a');
    expect(state.selectedUris.has('file:///root/a')).toBe(false);
  });

  it('resolves ticked URIs back to locations, including hardlink members', () => {
    let state = withDuplicateResults(
      withDuplicateScanStarted('scan-1', []),
      'scan-1',
      [
        group({
          hardlinkClusters: [
            {
              device: 1,
              inode: 2,
              locations: [location('file:///root/x'), location('file:///root/y')],
            },
          ],
        }),
      ],
      false,
      0,
    );
    state = withDuplicateSelectionToggled(state, 'file:///root/a');
    state = withDuplicateSelectionToggled(state, 'file:///root/x');
    expect(
      selectedLocations(state)
        .map((item) => item.uri)
        .sort(),
    ).toEqual(['file:///root/a', 'file:///root/x']);
  });

  it('clears ticks after a delete is dispatched', () => {
    let state = withDuplicateResults(
      withDuplicateScanStarted('scan-1', []),
      'scan-1',
      [group()],
      false,
      0,
    );
    state = withDuplicateSelectionToggled(state, 'file:///root/a');
    expect(withDuplicateSelectionCleared(state).selectedUris.size).toBe(0);
  });

  it('sums reclaimable bytes across groups', () => {
    const state = withDuplicateResults(
      withDuplicateScanStarted('scan-1', []),
      'scan-1',
      [group({ reclaimableBytes: 100 }), group({ fullHash: 'def', reclaimableBytes: 250 })],
      false,
      0,
    );
    expect(totalReclaimableBytes(state)).toBe(350);
  });

  it('refuses to let the last surviving copy be ticked', () => {
    let state = withDuplicateResults(
      withDuplicateScanStarted('scan-1', []),
      'scan-1',
      [group()],
      false,
      0,
    );
    expect(wouldDeleteEveryCopy(state, 'file:///root/a')).toBe(false);
    state = withDuplicateSelectionToggled(state, 'file:///root/a');
    // Ticking the second one too would leave nothing behind.
    expect(wouldDeleteEveryCopy(state, 'file:///root/b')).toBe(true);
  });

  it('counts a hardlink cluster as one surviving copy', () => {
    const state = withDuplicateResults(
      withDuplicateScanStarted('scan-1', []),
      'scan-1',
      [
        group({
          distinctLocations: [location('file:///root/a')],
          hardlinkClusters: [
            {
              device: 1,
              inode: 2,
              locations: [location('file:///root/x'), location('file:///root/y')],
            },
          ],
        }),
      ],
      false,
      0,
    );
    // The cluster still holds the content, so removing the distinct copy is fine.
    expect(wouldDeleteEveryCopy(state, 'file:///root/a')).toBe(false);
  });

  it('starts empty', () => {
    const state = initialDuplicateState();
    expect(state.groups).toEqual([]);
    expect(state.selectedUris.size).toBe(0);
  });
});
