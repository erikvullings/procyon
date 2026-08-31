import { meiosisSetup } from 'meiosis-setup';
import type { Patch } from 'meiosis-setup/types';
import m from 'mithril';

import type { AppState } from './model';
import type { AppPatch } from './patch';

/** Platform scheduling boundary used by every state producer. */
export type RequestFrame = (callback: FrameRequestCallback) => number;

export interface AppStoreOptions {
  readonly requestFrame?: RequestFrame;
  readonly redraw?: () => void;
}

export type StateSelector<Selected> = (state: AppState) => Selected;
export type StateListener<Selected> = (selected: Selected) => void;

/** Public interface consumed by actions and Mithril components. */
export interface AppStore {
  getState(): AppState;
  update(patch: AppPatch): void;
  subscribe<Selected>(
    selector: StateSelector<Selected>,
    listener: StateListener<Selected>,
  ): () => void;
}

/** Applies one or more immutable patches immediately. */
export function applyAppPatches(state: AppState, ...patches: readonly AppPatch[]): AppState {
  const cells = meiosisSetup<AppState>({ app: { initial: state } });
  cells().update(patches as Patch<AppState>);
  return cells().getState();
}

/**
 * Creates the Meiosis state loop. Updates queued in one animation frame are
 * merged into one snapshot publication followed by one Mithril redraw.
 */
export function createAppStore(initialState: AppState, options: AppStoreOptions = {}): AppStore {
  const requestFrame = options.requestFrame ?? requestAnimationFrame;
  const redraw = options.redraw ?? m.redraw;
  const cells = meiosisSetup<AppState>({ app: { initial: initialState } });
  let pending: AppPatch[] = [];
  let framePending = false;

  function flush(): void {
    framePending = false;
    const patches = pending;
    pending = [];
    cells().update(patches as Patch<AppState>);
    redraw();
  }

  return {
    getState: () => cells().getState(),
    update: (patch) => {
      pending.push(patch);
      if (!framePending) {
        framePending = true;
        requestFrame(flush);
      }
    },
    subscribe: (selector, listener) => {
      let selected = selector(cells().state);
      const subscription = cells.map((cell) => {
        const next = selector(cell.state);
        if (!Object.is(next, selected)) {
          selected = next;
          listener(next);
        }
        return next;
      });
      return () => subscription.end(true);
    },
  };
}
