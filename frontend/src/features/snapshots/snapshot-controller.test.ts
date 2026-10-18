import { describe, expect, it, vi } from 'vitest';
import type { SnapshotJobPage } from '../../models';
import { createSnapshotController, type SnapshotControllerContext } from './snapshot-controller';
import {
  childLocation,
  defaultSnapshotName,
  INITIAL_SNAPSHOT_PANEL_STATE,
  isSnapshotFileName,
  type SnapshotPanelState,
  snapshotFileName,
} from './snapshot-state';

const root = { providerId: 'local', uri: 'file:///data' };

function page(overrides: Partial<SnapshotJobPage> = {}): SnapshotJobPage {
  return {
    jobId: 'j1',
    kind: 'compare',
    phase: 'done',
    entriesFound: 3,
    filesProcessed: 3,
    bytesHashed: 0,
    hasChecksums: true,
    root,
    counts: { added: 1, removed: 0, changed: 0, corrupted: 0, unchanged: 2 },
    offset: 0,
    limit: 500,
    total: 1,
    entries: [{ kind: 'added', path: 'b.txt', isDirectory: false }],
    isComplete: true,
    isCancelled: false,
    hasMore: false,
    ...overrides,
  };
}

function setup(pages: SnapshotJobPage[]) {
  let state: SnapshotPanelState = INITIAL_SNAPSHOT_PANEL_STATE;
  const refreshPanes = vi.fn();
  const context: SnapshotControllerContext = {
    getClient: () => ({
      startSnapshotJob: vi.fn(() => Promise.resolve({ jobId: 'j1' })),
      getSnapshotJob: vi.fn(() => Promise.resolve(pages.shift() ?? page())),
      cancelSnapshotJob: vi.fn(() => Promise.resolve()),
    }),
    getState: () => state,
    setState: (next) => {
      state = next;
    },
    toast: vi.fn(),
    redraw: vi.fn(),
    refreshPanes,
  };
  return { controller: createSnapshotController(context), state: () => state, refreshPanes };
}

describe('snapshot names', () => {
  it('recognises, defaults and completes snapshot file names', () => {
    expect(isSnapshotFileName('Data.PROCYON-SNAPSHOT')).toBe(true);
    expect(isSnapshotFileName('data.txt')).toBe(false);
    expect(defaultSnapshotName('Photos')).toBe('Photos.procyon-snapshot');
    expect(defaultSnapshotName('  ')).toBe('snapshot.procyon-snapshot');
    expect(snapshotFileName('backup')).toBe('backup.procyon-snapshot');
    expect(snapshotFileName('backup.procyon-snapshot')).toBe('backup.procyon-snapshot');
    expect(childLocation(root, 'a b.procyon-snapshot').uri).toBe(
      'file:///data/a%20b.procyon-snapshot',
    );
  });
});

describe('snapshot controller', () => {
  it('collects the differences of a finished comparison', async () => {
    const { controller, state, refreshPanes } = setup([page()]);
    await controller.start({
      type: 'compare',
      snapshot: root,
      target: root,
    });
    expect(state().page?.isComplete).toBe(true);
    expect(state().entries.map((entry) => entry.path)).toEqual(['b.txt']);
    expect(refreshPanes).not.toHaveBeenCalled();
  });

  it('refreshes the panes after a snapshot file was written', async () => {
    const { controller, refreshPanes } = setup([page({ kind: 'create', entries: [] })]);
    await controller.start({
      type: 'create',
      root,
      destination: root,
      withChecksums: true,
    });
    expect(refreshPanes).toHaveBeenCalledOnce();
  });

  it('close resets the panel', async () => {
    const { controller, state } = setup([page()]);
    await controller.start({ type: 'compare', snapshot: root, target: root });
    controller.close();
    expect(state().jobId).toBeUndefined();
  });
});
