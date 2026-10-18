/**
 * Pure state and transitions for the audio queue (task 0262). The state lives in the Meiosis
 * tree (`AppState.player`, ADR 0007); every transition here is a pure function of the previous
 * state so ordering, shuffle and repeat rules are testable without an `<audio>` element.
 */
import type { Location } from '../../models/location';

export type RepeatMode = 'off' | 'all' | 'one';

export interface QueueItem {
  readonly id: string;
  readonly title: string;
  readonly location: Location;
  readonly durationSeconds: number | undefined;
}

export interface PlayerState {
  /** Tracks in the order the user arranged them. */
  readonly queue: readonly QueueItem[];
  /** Playback order while shuffle is on (ids of `queue`); empty otherwise. */
  readonly shuffleOrder: readonly string[];
  readonly currentId: string | undefined;
  /** The user's intent; the audio element follows it. */
  readonly playing: boolean;
  readonly positionSeconds: number;
  readonly durationSeconds: number | undefined;
  readonly volume: number;
  readonly muted: boolean;
  readonly shuffle: boolean;
  readonly repeat: RepeatMode;
  /** Ids that could not be loaded and were skipped; shown as a note. */
  readonly unavailableIds: readonly string[];
  readonly queueOpen: boolean;
  /** Bumped by {@link seekTo} so the controller can apply a user seek exactly once. */
  readonly seekRequest: { readonly seconds: number; readonly nonce: number } | undefined;
}

export type RandomSource = () => number;

export function createInitialPlayerState(): PlayerState {
  return {
    queue: [],
    shuffleOrder: [],
    currentId: undefined,
    playing: false,
    positionSeconds: 0,
    durationSeconds: undefined,
    volume: 1,
    muted: false,
    shuffle: false,
    repeat: 'off',
    unavailableIds: [],
    queueOpen: false,
    seekRequest: undefined,
  };
}

/** Ids in the order they will be played. */
export function playbackOrder(state: PlayerState): readonly string[] {
  return state.shuffle ? state.shuffleOrder : state.queue.map((item) => item.id);
}

export function currentItem(state: PlayerState): QueueItem | undefined {
  return state.queue.find((item) => item.id === state.currentId);
}

function shuffled(ids: readonly string[], first: string | undefined, random: RandomSource) {
  const rest = ids.filter((id) => id !== first);
  for (let index = rest.length - 1; index > 0; index -= 1) {
    const swap = Math.floor(random() * (index + 1));
    const held = rest[index];
    const other = rest[swap];
    if (held !== undefined && other !== undefined) {
      rest[index] = other;
      rest[swap] = held;
    }
  }
  return first === undefined ? rest : [first, ...rest];
}

function withOrder(state: PlayerState, random: RandomSource): PlayerState {
  return state.shuffle
    ? {
        ...state,
        shuffleOrder: shuffled(
          state.queue.map((item) => item.id),
          state.currentId,
          random,
        ),
      }
    : { ...state, shuffleOrder: [] };
}

function freshTrack(state: PlayerState): PlayerState {
  return { ...state, positionSeconds: 0, durationSeconds: undefined, seekRequest: undefined };
}

/** Replaces the queue and starts playing `startId` (default: first in playback order). */
export function playItems(
  state: PlayerState,
  items: readonly QueueItem[],
  startId: string | undefined,
  random: RandomSource = Math.random,
): PlayerState {
  if (items.length === 0) return state;
  const first = items.find((item) => item.id === startId)?.id ?? items[0]?.id;
  return withOrder(
    freshTrack({
      ...state,
      queue: items,
      currentId: first,
      playing: true,
      unavailableIds: [],
    }),
    random,
  );
}

/** Appends tracks to the end of the queue; starts playback only if nothing is loaded. */
export function enqueue(state: PlayerState, items: readonly QueueItem[]): PlayerState {
  if (items.length === 0) return state;
  const known = new Set(state.queue.map((item) => item.id));
  const added = items.filter((item) => !known.has(item.id));
  const queue = [...state.queue, ...added];
  const shuffleOrder = state.shuffle
    ? [...state.shuffleOrder, ...added.map((item) => item.id)]
    : state.shuffleOrder;
  const idle = state.currentId === undefined;
  return idle
    ? freshTrack({ ...state, queue, shuffleOrder, currentId: added[0]?.id, playing: true })
    : { ...state, queue, shuffleOrder };
}

/** Inserts tracks right after the current one. */
export function enqueueNext(state: PlayerState, items: readonly QueueItem[]): PlayerState {
  if (state.currentId === undefined) return enqueue(state, items);
  const known = new Set(state.queue.map((item) => item.id));
  const added = items.filter((item) => !known.has(item.id));
  if (added.length === 0) return state;
  const insert = (ids: readonly string[]): string[] => {
    const at = ids.indexOf(state.currentId ?? '') + 1;
    return [...ids.slice(0, at), ...added.map((item) => item.id), ...ids.slice(at)];
  };
  const at = state.queue.findIndex((item) => item.id === state.currentId) + 1;
  return {
    ...state,
    queue: [...state.queue.slice(0, at), ...added, ...state.queue.slice(at)],
    shuffleOrder: state.shuffle ? insert(state.shuffleOrder) : state.shuffleOrder,
  };
}

export function removeFromQueue(state: PlayerState, id: string): PlayerState {
  if (!state.queue.some((item) => item.id === id)) return state;
  if (id === state.currentId) {
    const following = advance(state, 1, 'user');
    const base = following.currentId === id ? { ...following, currentId: undefined } : following;
    return {
      ...base,
      playing: base.currentId === undefined ? false : base.playing,
      queue: base.queue.filter((item) => item.id !== id),
      shuffleOrder: base.shuffleOrder.filter((other) => other !== id),
      unavailableIds: base.unavailableIds.filter((other) => other !== id),
    };
  }
  return {
    ...state,
    queue: state.queue.filter((item) => item.id !== id),
    shuffleOrder: state.shuffleOrder.filter((other) => other !== id),
    unavailableIds: state.unavailableIds.filter((other) => other !== id),
  };
}

export function clearQueue(state: PlayerState): PlayerState {
  return {
    ...state,
    queue: [],
    shuffleOrder: [],
    currentId: undefined,
    playing: false,
    positionSeconds: 0,
    durationSeconds: undefined,
    unavailableIds: [],
    queueOpen: false,
    seekRequest: undefined,
  };
}

export function jumpTo(state: PlayerState, id: string): PlayerState {
  if (!state.queue.some((item) => item.id === id)) return state;
  return freshTrack({ ...state, currentId: id, playing: true });
}

/**
 * Moves to the neighbouring playable track. `ended` honours repeat-one and stops after the last
 * track unless repeat-all wraps; a user skip always moves (wrapping when repeat is on).
 * Tracks marked unavailable are passed over.
 */
export function advance(
  state: PlayerState,
  direction: 1 | -1,
  reason: 'user' | 'ended',
): PlayerState {
  const order = playbackOrder(state);
  if (order.length === 0 || state.currentId === undefined) return state;
  if (reason === 'ended' && state.repeat === 'one') {
    return freshTrack({ ...state, playing: true });
  }
  const unavailable = new Set(state.unavailableIds);
  const start = order.indexOf(state.currentId);
  const wraps = state.repeat === 'all' || reason === 'user';
  for (let step = 1; step <= order.length; step += 1) {
    let index = start + direction * step;
    if (index < 0 || index >= order.length) {
      if (!wraps) break;
      index = (index + order.length) % order.length;
    }
    const candidate = order[index];
    if (candidate === undefined || unavailable.has(candidate)) continue;
    if (candidate === state.currentId && reason === 'ended' && state.repeat !== 'all') break;
    return freshTrack({ ...state, currentId: candidate, playing: true });
  }
  return reason === 'ended'
    ? { ...state, playing: false, positionSeconds: 0, seekRequest: undefined }
    : state;
}

/** A track failed to load: remember it and move on (or stop when nothing playable remains). */
export function markUnavailable(state: PlayerState, id: string): PlayerState {
  if (state.unavailableIds.includes(id)) return state;
  const marked = { ...state, unavailableIds: [...state.unavailableIds, id] };
  if (id !== state.currentId) return marked;
  const next = advance(marked, 1, 'ended');
  return next.currentId === id ? { ...next, playing: false } : next;
}

export function setPlaying(state: PlayerState, playing: boolean): PlayerState {
  return state.currentId === undefined ? state : { ...state, playing };
}

export function setShuffle(
  state: PlayerState,
  shuffle: boolean,
  random: RandomSource = Math.random,
): PlayerState {
  return withOrder({ ...state, shuffle }, random);
}

const REPEAT_CYCLE: Readonly<Record<RepeatMode, RepeatMode>> = {
  off: 'all',
  all: 'one',
  one: 'off',
};

export function cycleRepeat(state: PlayerState): PlayerState {
  return { ...state, repeat: REPEAT_CYCLE[state.repeat] };
}

export function setVolume(state: PlayerState, volume: number): PlayerState {
  const clamped = Math.min(1, Math.max(0, Number.isFinite(volume) ? volume : 1));
  return { ...state, volume: clamped, muted: clamped === 0 ? state.muted : false };
}

export function setMuted(state: PlayerState, muted: boolean): PlayerState {
  return { ...state, muted };
}

export function seekTo(state: PlayerState, seconds: number): PlayerState {
  const upper = state.durationSeconds ?? Number.POSITIVE_INFINITY;
  const clamped = Math.min(upper, Math.max(0, seconds));
  return {
    ...state,
    positionSeconds: clamped,
    seekRequest: { seconds: clamped, nonce: (state.seekRequest?.nonce ?? 0) + 1 },
  };
}

export function reportProgress(
  state: PlayerState,
  positionSeconds: number,
  durationSeconds: number | undefined,
): PlayerState {
  return { ...state, positionSeconds, durationSeconds: durationSeconds ?? state.durationSeconds };
}

export function setQueueOpen(state: PlayerState, queueOpen: boolean): PlayerState {
  return { ...state, queueOpen };
}
