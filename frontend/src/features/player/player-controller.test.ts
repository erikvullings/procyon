import { describe, expect, it, vi } from 'vitest';
import {
  createPlayerController,
  type MediaSessionLike,
  type PlayerAudio,
} from './player-controller';
import { createInitialPlayerState, type PlayerState, type QueueItem } from './player-state';

class FakeAudio extends EventTarget {
  src = '';
  currentTime = 0;
  duration = Number.NaN;
  volume = 1;
  muted = false;
  playCalls = 0;
  pauseCalls = 0;
  play = vi.fn(() => {
    this.playCalls += 1;
    return Promise.resolve();
  });
  pause = vi.fn(() => {
    this.pauseCalls += 1;
  });
  removeAttribute = vi.fn((_name: string) => {
    this.src = '';
  });
  load = vi.fn();
}

function item(id: string): QueueItem {
  return {
    id,
    title: id,
    location: { providerId: 'local', uri: `file:///m/${id}.mp3` },
    durationSeconds: undefined,
  };
}

function setup() {
  const audio = new FakeAudio();
  let state = createInitialPlayerState();
  const mediaUrl = vi.fn((location: { uri: string }) => Promise.resolve(`media:${location.uri}`));
  const handlers = new Map<string, (details: { seekTime?: number }) => void>();
  const session: MediaSessionLike = {
    metadata: null,
    playbackState: 'none',
    setActionHandler: (action, handler) => {
      if (handler === null) handlers.delete(action);
      else handlers.set(action, handler);
    },
  };
  const controller = createPlayerController({
    client: { mediaUrl },
    getState: () => state,
    update: (next: PlayerState) => {
      state = next;
    },
    createAudio: () => audio as unknown as PlayerAudio,
    mediaSession: session,
    random: () => 0,
  });
  return { audio, controller, mediaUrl, handlers, session, state: () => state };
}

const flush = (): Promise<void> => new Promise((resolve) => setTimeout(resolve, 0));

describe('createPlayerController', () => {
  it('loads the streaming URL of the current track and plays it', async () => {
    const { audio, controller, state } = setup();
    controller.playItems([item('a'), item('b')]);
    await flush();
    expect(audio.src).toBe('media:file:///m/a.mp3');
    expect(audio.playCalls).toBe(1);
    expect(state().playing).toBe(true);
  });

  it('advances to the next track when one ends', async () => {
    const { audio, controller, state } = setup();
    controller.playItems([item('a'), item('b')]);
    await flush();
    audio.dispatchEvent(new Event('ended'));
    await flush();
    expect(state().currentId).toBe('b');
    expect(audio.src).toBe('media:file:///m/b.mp3');
  });

  it('skips a track that fails to load and records it', async () => {
    const { audio, controller, state } = setup();
    controller.playItems([item('a'), item('b')]);
    await flush();
    audio.dispatchEvent(new Event('error'));
    await flush();
    expect(state().unavailableIds).toEqual(['a']);
    expect(state().currentId).toBe('b');
  });

  it('skips a track whose URL cannot be built', async () => {
    const { controller, mediaUrl, state } = setup();
    mediaUrl.mockRejectedValueOnce(new Error('gone'));
    controller.playItems([item('a'), item('b')]);
    await flush();
    expect(state().unavailableIds).toEqual(['a']);
    expect(state().currentId).toBe('b');
  });

  it('pauses and resumes through the toggle and applies seeks and volume', async () => {
    const { audio, controller, state } = setup();
    controller.playItems([item('a')]);
    await flush();
    controller.togglePlay();
    expect(state().playing).toBe(false);
    expect(audio.pauseCalls).toBeGreaterThan(0);
    controller.seek(42);
    expect(audio.currentTime).toBe(42);
    controller.setVolume(0.25);
    expect(audio.volume).toBe(0.25);
  });

  it('previous restarts a track that has played for a while', async () => {
    const { audio, controller, state } = setup();
    controller.playItems([item('a'), item('b')]);
    await flush();
    controller.next();
    await flush();
    audio.currentTime = 30;
    audio.dispatchEvent(new Event('timeupdate'));
    controller.previous();
    expect(state().currentId).toBe('b');
    expect(audio.currentTime).toBe(0);
    controller.previous();
    expect(state().currentId).toBe('a');
  });

  it('replays the same track with repeat-one', async () => {
    const { audio, controller, state } = setup();
    controller.playItems([item('a'), item('b')]);
    controller.cycleRepeat();
    controller.cycleRepeat();
    expect(state().repeat).toBe('one');
    await flush();
    audio.playCalls = 0;
    audio.dispatchEvent(new Event('ended'));
    expect(state().currentId).toBe('a');
    expect(audio.playCalls).toBe(1);
  });

  it('wires Media Session handlers to the transport controls', async () => {
    const { controller, handlers, session, state } = setup();
    controller.playItems([item('a'), item('b')]);
    await flush();
    expect(session.playbackState).toBe('playing');
    handlers.get('nexttrack')?.({});
    expect(state().currentId).toBe('b');
    handlers.get('pause')?.({});
    expect(state().playing).toBe(false);
    handlers.get('seekto')?.({ seekTime: 12 });
    expect(state().positionSeconds).toBe(12);
    controller.dispose();
    expect(handlers.size).toBe(0);
  });

  it('empties the element when the queue is cleared', async () => {
    const { audio, controller, state } = setup();
    controller.playItems([item('a')]);
    await flush();
    controller.clear();
    expect(state().currentId).toBeUndefined();
    expect(audio.src).toBe('');
  });
});
