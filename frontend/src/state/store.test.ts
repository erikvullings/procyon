import { describe, expect, it, vi } from 'vitest';

import { createInitialAppState } from './model';
import { createAppStore } from './store';

describe('AppStore', () => {
  it('applies object patches without mutating the prior snapshot', () => {
    const frames: FrameRequestCallback[] = [];
    const redraw = vi.fn();
    const store = createAppStore(createInitialAppState('mock'), {
      requestFrame: (callback) => {
        frames.push(callback);
        return frames.length;
      },
      redraw,
    });
    const before = store.getState();

    store.update({ connection: { status: 'open' } });
    frames[0]?.(0);

    expect(store.getState().connection.status).toBe('open');
    expect(before.connection.status).toBe('closed');
    expect(store.getState()).not.toBe(before);
    expect(store.getState().runtime).toBe(before.runtime);
  });

  it('batches N updates into one state publication and one redraw in a frame', () => {
    const frames: FrameRequestCallback[] = [];
    const redraw = vi.fn();
    const store = createAppStore(createInitialAppState('mock'), {
      requestFrame: (callback) => {
        frames.push(callback);
        return frames.length;
      },
      redraw,
    });
    const listener = vi.fn();
    store.subscribe((state) => state.connection, listener);

    store.update({ connection: { status: 'connecting' } });
    store.update({ connection: { status: 'open' } });
    store.update({ connection: { lastEventId: 42 } });

    expect(frames).toHaveLength(1);
    expect(redraw).not.toHaveBeenCalled();
    frames[0]?.(0);
    expect(listener).toHaveBeenCalledTimes(1);
    expect(store.getState().connection).toEqual({ status: 'open', lastEventId: 42 });
    expect(redraw).toHaveBeenCalledTimes(1);
  });

  it('notifies a targeted subscription only when its selected slice changes', () => {
    const frames: FrameRequestCallback[] = [];
    const store = createAppStore(createInitialAppState('mock'), {
      requestFrame: (callback) => {
        frames.push(callback);
        return frames.length;
      },
      redraw: vi.fn(),
    });
    const listener = vi.fn();
    const unsubscribe = store.subscribe((state) => state.connection, listener);

    store.update({ runtime: { kind: 'tauri' } });
    frames.shift()?.(0);
    expect(listener).not.toHaveBeenCalled();

    store.update({ connection: { status: 'connecting' } });
    frames.shift()?.(1);
    expect(listener).toHaveBeenCalledTimes(1);

    unsubscribe();
    store.update({ connection: { status: 'open' } });
    frames.shift()?.(2);
    expect(listener).toHaveBeenCalledTimes(1);
  });
});
