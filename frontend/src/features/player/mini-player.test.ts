import m from 'mithril';
import { describe, expect, it, vi } from 'vitest';
import { MiniPlayer } from './mini-player';
import type { PlayerController } from './player-controller';
import {
  createInitialPlayerState,
  type PlayerState,
  playItems,
  type QueueItem,
} from './player-state';

function item(id: string): QueueItem {
  return {
    id,
    title: `Song ${id}`,
    location: { providerId: 'local', uri: `file:///m/${id}.mp3` },
    durationSeconds: 125,
  };
}

function controllerSpy(): PlayerController {
  return {
    playItems: vi.fn(),
    enqueue: vi.fn(),
    enqueueNext: vi.fn(),
    togglePlay: vi.fn(),
    next: vi.fn(),
    previous: vi.fn(),
    seek: vi.fn(),
    setVolume: vi.fn(),
    toggleMute: vi.fn(),
    toggleShuffle: vi.fn(),
    cycleRepeat: vi.fn(),
    jumpTo: vi.fn(),
    remove: vi.fn(),
    clear: vi.fn(),
    toggleQueue: vi.fn(),
    dispose: vi.fn(),
  };
}

function render(state: PlayerState, controller = controllerSpy()): HTMLElement {
  const root = document.createElement('div');
  m.render(root, m(MiniPlayer, { state, controller }));
  return root;
}

describe('MiniPlayer', () => {
  it('renders nothing while the queue is empty', () => {
    expect(render(createInitialPlayerState()).querySelector('.fm-mini-player')).toBeNull();
  });

  it('shows the current track and routes the transport buttons to the controller', () => {
    const controller = controllerSpy();
    const state = playItems(createInitialPlayerState(), [item('a'), item('b')], undefined);
    const root = render({ ...state, durationSeconds: 125, positionSeconds: 65 }, controller);

    expect(root.querySelector('.fm-player-title')?.textContent).toBe('Song a');
    expect(root.textContent).toContain('1:05');
    expect(root.textContent).toContain('2:05');
    (root.querySelector('[aria-label="Next track"]') as HTMLButtonElement).click();
    (root.querySelector('[aria-label="Pause"]') as HTMLButtonElement).click();
    expect(controller.next).toHaveBeenCalledTimes(1);
    expect(controller.togglePlay).toHaveBeenCalledTimes(1);
  });

  it('lists the queue when open, marks unavailable tracks and lets the user jump or remove', () => {
    const controller = controllerSpy();
    const base = playItems(createInitialPlayerState(), [item('a'), item('b')], undefined);
    const root = render({ ...base, queueOpen: true, unavailableIds: ['b'] }, controller);

    const rows = root.querySelectorAll('.fm-player-queue-row');
    expect(rows).toHaveLength(2);
    expect(rows[1]?.classList.contains('fm-player-queue-unavailable')).toBe(true);
    expect(root.querySelector('.fm-player-note')?.textContent).toContain('1 track could not');
    (rows[0]?.querySelector('.fm-player-queue-title') as HTMLButtonElement | null)?.click();
    (
      rows[1]?.querySelector('[aria-label="Remove from queue"]') as HTMLButtonElement | null
    )?.click();
    expect(controller.jumpTo).toHaveBeenCalledWith('a');
    expect(controller.remove).toHaveBeenCalledWith('b');
  });
});
