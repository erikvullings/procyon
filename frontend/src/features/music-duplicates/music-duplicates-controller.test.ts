import { describe, expect, it, vi } from 'vitest';
import type { MusicDuplicateGroup, MusicDuplicatePage } from '../../models';
import {
  createMusicDuplicatesController,
  type MusicDuplicatesControllerContext,
} from './music-duplicates-controller';
import {
  INITIAL_MUSIC_DUPLICATES_STATE,
  type MusicDuplicatesState,
} from './music-duplicates-state';

const group: MusicDuplicateGroup = {
  confidence: 'likely',
  files: [
    { location: { providerId: 'local', uri: 'file:///m/a.flac' }, size: 30, lossless: true },
    { location: { providerId: 'local', uri: 'file:///m/a.mp3' }, size: 5, lossless: false },
  ],
  keepIndex: 0,
  reclaimableBytes: 5,
};

function page(overrides: Partial<MusicDuplicatePage> = {}): MusicDuplicatePage {
  return {
    scanId: 's1',
    roots: [],
    phase: 'done',
    filesFound: 2,
    filesProcessed: 2,
    unreadableTags: 1,
    offset: 0,
    limit: 500,
    total: 1,
    groups: [group],
    isComplete: true,
    isCancelled: false,
    hasMore: false,
    ...overrides,
  };
}

function setup(pages: MusicDuplicatePage[]) {
  let state: MusicDuplicatesState = INITIAL_MUSIC_DUPLICATES_STATE;
  const getMusicDuplicateScan = vi.fn(() => Promise.resolve(pages.shift() ?? page()));
  const trash = vi.fn(() => Promise.resolve({} as never));
  const toast = vi.fn();
  const context: MusicDuplicatesControllerContext = {
    getClient: () => ({
      startMusicDuplicateScan: vi.fn(() => Promise.resolve({ scanId: 's1' })),
      getMusicDuplicateScan,
      cancelMusicDuplicateScan: vi.fn(() => Promise.resolve()),
    }),
    getOpsController: () => ({ trash }),
    getState: () => state,
    setState: (next) => {
      state = next;
    },
    toast,
    redraw: vi.fn(),
    refreshPanes: vi.fn(),
  };
  return { controller: createMusicDuplicatesController(context), state: () => state, trash, toast };
}

describe('music duplicates controller', () => {
  it('scans to completion and exposes the groups and unreadable-tag count', async () => {
    const { controller, state } = setup([page()]);
    await controller.start([{ providerId: 'local', uri: 'file:///m' }]);
    expect(state().isComplete).toBe(true);
    expect(state().groups).toHaveLength(1);
    expect(state().unreadableTags).toBe(1);
  });

  it('selects the suggested removals and trashes exactly those files', async () => {
    const { controller, state, trash } = setup([page()]);
    await controller.start([{ providerId: 'local', uri: 'file:///m' }]);

    controller.selectSuggestedRemovals();
    expect([...state().selectedUris]).toEqual(['file:///m/a.mp3']);
    await controller.removeSelected();

    expect(trash).toHaveBeenCalledWith([{ providerId: 'local', uri: 'file:///m/a.mp3' }]);
    expect(state().groups).toEqual([]);
    expect(state().selectedUris.size).toBe(0);
  });

  it('refuses to tick the last remaining copy', async () => {
    const { controller, state } = setup([page()]);
    await controller.start([{ providerId: 'local', uri: 'file:///m' }]);
    controller.toggle('file:///m/a.mp3');
    controller.toggle('file:///m/a.flac');
    expect([...state().selectedUris]).toEqual(['file:///m/a.mp3']);
    expect(controller.isLastCopy('file:///m/a.flac')).toBe(true);
  });

  it('close resets the panel', async () => {
    const { controller, state } = setup([page()]);
    await controller.start([{ providerId: 'local', uri: 'file:///m' }]);
    controller.close();
    expect(state().scanId).toBeUndefined();
  });
});
