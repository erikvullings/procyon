import { describe, expect, it } from 'vitest';
import {
  advance,
  clearQueue,
  createInitialPlayerState,
  currentItem,
  cycleRepeat,
  enqueue,
  enqueueNext,
  jumpTo,
  markUnavailable,
  type PlayerState,
  playbackOrder,
  playItems,
  type QueueItem,
  removeFromQueue,
  seekTo,
  setPlaying,
  setShuffle,
  setVolume,
} from './player-state';

function item(id: string): QueueItem {
  return {
    id,
    title: id,
    location: { providerId: 'local', uri: `file:///m/${id}.mp3` },
    durationSeconds: undefined,
  };
}

const ABC = [item('a'), item('b'), item('c')];
const never = (): number => 0;

function started(overrides: Partial<PlayerState> = {}): PlayerState {
  return { ...playItems(createInitialPlayerState(), ABC, undefined, never), ...overrides };
}

describe('queue building', () => {
  it('starts at the requested item and plays', () => {
    const state = playItems(createInitialPlayerState(), ABC, 'b', never);
    expect(state.currentId).toBe('b');
    expect(state.playing).toBe(true);
  });

  it('ignores an empty replacement', () => {
    const state = started();
    expect(playItems(state, [], undefined)).toBe(state);
  });

  it('enqueue starts an idle player and otherwise only appends, skipping duplicates', () => {
    const idle = enqueue(createInitialPlayerState(), ABC);
    expect(idle.currentId).toBe('a');
    expect(idle.playing).toBe(true);
    const busy = enqueue(started(), [item('c'), item('d')]);
    expect(busy.queue.map((entry) => entry.id)).toEqual(['a', 'b', 'c', 'd']);
    expect(busy.currentId).toBe('a');
  });

  it('enqueueNext inserts after the current track', () => {
    const state = enqueueNext(jumpTo(started(), 'b'), [item('x')]);
    expect(state.queue.map((entry) => entry.id)).toEqual(['a', 'b', 'x', 'c']);
  });

  it('removes tracks and moves on when the current one is removed', () => {
    const state = removeFromQueue(started(), 'a');
    expect(state.queue.map((entry) => entry.id)).toEqual(['b', 'c']);
    expect(state.currentId).toBe('b');
    const lone = removeFromQueue(
      playItems(createInitialPlayerState(), [item('z')], undefined),
      'z',
    );
    expect(lone.queue).toEqual([]);
    expect(lone.currentId).toBeUndefined();
    expect(lone.playing).toBe(false);
  });

  it('clears everything', () => {
    expect(currentItem(clearQueue(started()))).toBeUndefined();
  });
});

describe('advance', () => {
  it('moves forward and back on a user skip, wrapping around', () => {
    const state = started();
    expect(advance(state, 1, 'user').currentId).toBe('b');
    expect(advance(state, -1, 'user').currentId).toBe('c');
  });

  it('stops after the last track when a track ends and repeat is off', () => {
    const last = jumpTo(started(), 'c');
    const ended = advance(last, 1, 'ended');
    expect(ended.playing).toBe(false);
    expect(ended.currentId).toBe('c');
  });

  it('wraps on end with repeat-all and replays with repeat-one', () => {
    const last = jumpTo(started({ repeat: 'all' }), 'c');
    expect(advance(last, 1, 'ended').currentId).toBe('a');
    const one = started({ repeat: 'one' });
    const replay = advance(jumpTo(one, 'b'), 1, 'ended');
    expect(replay.currentId).toBe('b');
    expect(replay.playing).toBe(true);
  });

  it('a lone track with repeat-all replays itself', () => {
    const lone = playItems(createInitialPlayerState(), [item('a')], undefined, never);
    const state = { ...lone, repeat: 'all' as const };
    expect(advance(state, 1, 'ended').currentId).toBe('a');
    expect(advance(state, 1, 'ended').playing).toBe(true);
  });

  it('passes over unavailable tracks', () => {
    const state = started({ unavailableIds: ['b'] });
    expect(advance(state, 1, 'ended').currentId).toBe('c');
  });
});

describe('unavailable tracks', () => {
  it('records the failure and skips ahead', () => {
    const state = markUnavailable(started(), 'a');
    expect(state.unavailableIds).toEqual(['a']);
    expect(state.currentId).toBe('b');
  });

  it('stops when nothing playable remains', () => {
    const lone = playItems(createInitialPlayerState(), [item('a')], undefined, never);
    const state = markUnavailable(lone, 'a');
    expect(state.playing).toBe(false);
  });
});

describe('shuffle', () => {
  it('keeps the current track first and permutes the rest', () => {
    const state = setShuffle(jumpTo(started(), 'b'), true, () => 0.99);
    expect(playbackOrder(state)[0]).toBe('b');
    expect([...playbackOrder(state)].sort()).toEqual(['a', 'b', 'c']);
  });

  it('follows the shuffled order for next and previous', () => {
    const state = setShuffle(started(), true, never);
    const order = playbackOrder(state);
    const next = advance(state, 1, 'user');
    expect(next.currentId).toBe(order[1]);
    expect(advance(next, -1, 'user').currentId).toBe(order[0]);
  });

  it('turning shuffle off restores queue order', () => {
    const off = setShuffle(setShuffle(started(), true, never), false, never);
    expect(playbackOrder(off)).toEqual(['a', 'b', 'c']);
    expect(off.shuffleOrder).toEqual([]);
  });
});

describe('controls', () => {
  it('cycles repeat modes', () => {
    const modes = [createInitialPlayerState()];
    for (let i = 0; i < 3; i += 1) {
      const last = modes.at(-1);
      if (last) modes.push(cycleRepeat(last));
    }
    expect(modes.map((state) => state.repeat)).toEqual(['off', 'all', 'one', 'off']);
  });

  it('clamps volume and unmutes on a raised volume', () => {
    expect(setVolume(createInitialPlayerState(), 4).volume).toBe(1);
    expect(setVolume({ ...createInitialPlayerState(), muted: true }, 0.4).muted).toBe(false);
  });

  it('seek requests are numbered and clamped to the duration', () => {
    const first = seekTo({ ...started(), durationSeconds: 100 }, 250);
    expect(first.seekRequest).toEqual({ seconds: 100, nonce: 1 });
    expect(seekTo(first, -5).seekRequest).toEqual({ seconds: 0, nonce: 2 });
  });

  it('cannot play an empty queue', () => {
    expect(setPlaying(createInitialPlayerState(), true).playing).toBe(false);
  });
});
