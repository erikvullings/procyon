/**
 * Drives one `<audio>` element from the queue state in the Meiosis tree (task 0262). Every
 * public method is a pure transition from `player-state.ts` followed by reconciliation of the
 * element (source, play/pause, volume, seek) and the Media Session. The element's own events
 * (progress, ended, error) feed back as transitions, so the tree stays authoritative.
 */
import type { FileManagerClient } from '../../api/client/file-manager-client';
import {
  advance,
  clearQueue,
  currentItem,
  cycleRepeat,
  enqueue,
  enqueueNext,
  jumpTo,
  markUnavailable,
  type PlayerState,
  playItems,
  type QueueItem,
  type RandomSource,
  removeFromQueue,
  reportProgress,
  seekTo,
  setMuted,
  setPlaying,
  setQueueOpen,
  setShuffle,
  setVolume,
} from './player-state';

export type PlayerAudio = Pick<
  HTMLAudioElement,
  | 'src'
  | 'currentTime'
  | 'duration'
  | 'volume'
  | 'muted'
  | 'play'
  | 'pause'
  | 'addEventListener'
  | 'removeEventListener'
  | 'removeAttribute'
  | 'load'
>;

type MediaSessionAction = 'play' | 'pause' | 'previoustrack' | 'nexttrack' | 'seekto';

/** The slice of `navigator.mediaSession` the player uses. */
export interface MediaSessionLike {
  metadata: unknown;
  playbackState: 'none' | 'paused' | 'playing';
  setActionHandler(
    action: MediaSessionAction,
    handler: ((details: { seekTime?: number }) => void) | null,
  ): void;
  setPositionState?(state?: { duration: number; position: number; playbackRate: number }): void;
}

export interface PlayerControllerOptions {
  readonly client: Pick<FileManagerClient, 'mediaUrl'>;
  readonly getState: () => PlayerState;
  readonly update: (state: PlayerState) => void;
  readonly createAudio?: () => PlayerAudio;
  readonly mediaSession?: MediaSessionLike | undefined;
  readonly random?: RandomSource;
}

export interface PlayerController {
  playItems(items: readonly QueueItem[], startId?: string): void;
  enqueue(items: readonly QueueItem[]): void;
  enqueueNext(items: readonly QueueItem[]): void;
  togglePlay(): void;
  next(): void;
  previous(): void;
  seek(seconds: number): void;
  setVolume(volume: number): void;
  toggleMute(): void;
  toggleShuffle(): void;
  cycleRepeat(): void;
  jumpTo(id: string): void;
  remove(id: string): void;
  clear(): void;
  toggleQueue(): void;
  dispose(): void;
}

/** Restart a track instead of going to the previous one once it has played this long. */
const RESTART_AFTER_SECONDS = 3;
/** Progress events closer than this to the stored position are not worth a state update. */
const PROGRESS_EPSILON_SECONDS = 0.4;

function defaultMediaSession(): MediaSessionLike | undefined {
  const navigatorLike = globalThis.navigator as Navigator | undefined;
  return navigatorLike !== undefined && 'mediaSession' in navigatorLike
    ? (navigatorLike.mediaSession as unknown as MediaSessionLike)
    : undefined;
}

export function createPlayerController(options: PlayerControllerOptions): PlayerController {
  const { client, getState, update } = options;
  const random = options.random ?? Math.random;
  const audio: PlayerAudio = (options.createAudio ?? (() => new Audio()))();
  const session = 'mediaSession' in options ? options.mediaSession : defaultMediaSession();
  let loadToken = 0;
  let appliedSeekNonce = 0;
  let disposed = false;

  function commit(next: PlayerState): void {
    const previous = getState();
    if (next === previous) return;
    update(next);
    reconcile(previous, next);
  }

  function applyPlayback(state: PlayerState): void {
    if (state.playing) {
      audio.play().catch(() => {
        // Autoplay was refused or the source failed; the error event handles the latter.
        const current = getState();
        if (current.playing && current.currentId === state.currentId) {
          update(setPlaying(current, false));
        }
      });
    } else {
      audio.pause();
    }
  }

  function loadCurrent(state: PlayerState): void {
    loadToken += 1;
    const token = loadToken;
    const item = currentItem(state);
    audio.pause();
    if (item === undefined) {
      audio.removeAttribute('src');
      audio.load();
      return;
    }
    client
      .mediaUrl(item.location)
      .then((url) => {
        if (disposed || token !== loadToken) return;
        audio.src = url;
        applyPlayback(getState());
      })
      .catch(() => {
        if (disposed || token !== loadToken) return;
        commit(markUnavailable(getState(), item.id));
      });
  }

  function reconcile(previous: PlayerState, next: PlayerState): void {
    audio.volume = next.volume;
    audio.muted = next.muted;
    if (next.currentId !== previous.currentId) {
      loadCurrent(next);
    } else if (next.playing !== previous.playing && next.currentId !== undefined) {
      applyPlayback(next);
    }
    if (next.seekRequest !== undefined && next.seekRequest.nonce !== appliedSeekNonce) {
      appliedSeekNonce = next.seekRequest.nonce;
      audio.currentTime = next.seekRequest.seconds;
    }
    syncMediaSession(next);
  }

  function syncMediaSession(state: PlayerState): void {
    if (session === undefined) return;
    const item = currentItem(state);
    session.playbackState = item === undefined ? 'none' : state.playing ? 'playing' : 'paused';
    if (item === undefined) {
      session.metadata = null;
      return;
    }
    if (typeof MediaMetadata !== 'undefined') {
      session.metadata = new MediaMetadata({ title: item.title, artist: 'Procyon' });
    }
    if (state.durationSeconds !== undefined && state.durationSeconds > 0) {
      try {
        session.setPositionState?.({
          duration: state.durationSeconds,
          position: Math.min(state.positionSeconds, state.durationSeconds),
          playbackRate: 1,
        });
      } catch {
        // Some webviews reject inconsistent position state; the display is cosmetic.
      }
    }
  }

  function currentDuration(): number | undefined {
    return Number.isFinite(audio.duration) && audio.duration > 0 ? audio.duration : undefined;
  }

  const onTimeUpdate = (): void => {
    const state = getState();
    if (
      Math.abs(audio.currentTime - state.positionSeconds) < PROGRESS_EPSILON_SECONDS &&
      state.durationSeconds !== undefined
    ) {
      return;
    }
    update(reportProgress(state, audio.currentTime, currentDuration()));
  };
  const onMetadata = (): void => {
    const state = getState();
    const next = reportProgress(state, state.positionSeconds, currentDuration());
    update(next);
    syncMediaSession(next);
  };
  const onEnded = (): void => {
    const state = getState();
    const next = advance(state, 1, 'ended');
    if (next.currentId === state.currentId && next.playing) {
      commit(next);
      audio.currentTime = 0;
      applyPlayback(next);
      return;
    }
    commit(next);
  };
  const onError = (): void => {
    const state = getState();
    if (state.currentId !== undefined && audio.src !== '') {
      commit(markUnavailable(state, state.currentId));
    }
  };
  audio.addEventListener('timeupdate', onTimeUpdate);
  audio.addEventListener('loadedmetadata', onMetadata);
  audio.addEventListener('durationchange', onMetadata);
  audio.addEventListener('ended', onEnded);
  audio.addEventListener('error', onError);

  const controller: PlayerController = {
    playItems: (items, startId) => commit(playItems(getState(), items, startId, random)),
    enqueue: (items) => commit(enqueue(getState(), items)),
    enqueueNext: (items) => commit(enqueueNext(getState(), items)),
    togglePlay: () => commit(setPlaying(getState(), !getState().playing)),
    next: () => commit(advance(getState(), 1, 'user')),
    previous: () => {
      const state = getState();
      if (state.positionSeconds > RESTART_AFTER_SECONDS) commit(seekTo(state, 0));
      else commit(advance(state, -1, 'user'));
    },
    seek: (seconds) => commit(seekTo(getState(), seconds)),
    setVolume: (volume) => commit(setVolume(getState(), volume)),
    toggleMute: () => commit(setMuted(getState(), !getState().muted)),
    toggleShuffle: () => commit(setShuffle(getState(), !getState().shuffle, random)),
    cycleRepeat: () => commit(cycleRepeat(getState())),
    jumpTo: (id) => commit(jumpTo(getState(), id)),
    remove: (id) => commit(removeFromQueue(getState(), id)),
    clear: () => commit(clearQueue(getState())),
    toggleQueue: () => commit(setQueueOpen(getState(), !getState().queueOpen)),
    dispose: () => {
      disposed = true;
      audio.removeEventListener('timeupdate', onTimeUpdate);
      audio.removeEventListener('loadedmetadata', onMetadata);
      audio.removeEventListener('durationchange', onMetadata);
      audio.removeEventListener('ended', onEnded);
      audio.removeEventListener('error', onError);
      audio.pause();
      for (const action of ['play', 'pause', 'previoustrack', 'nexttrack', 'seekto'] as const) {
        try {
          session?.setActionHandler(action, null);
        } catch {
          // Unsupported action in this webview.
        }
      }
    },
  };

  const handlers: Readonly<Record<MediaSessionAction, (details: { seekTime?: number }) => void>> = {
    play: () => commit(setPlaying(getState(), true)),
    pause: () => commit(setPlaying(getState(), false)),
    previoustrack: () => controller.previous(),
    nexttrack: () => controller.next(),
    seekto: (details) => {
      if (details.seekTime !== undefined) controller.seek(details.seekTime);
    },
  };
  for (const action of Object.keys(handlers) as MediaSessionAction[]) {
    try {
      session?.setActionHandler(action, handlers[action]);
    } catch {
      // Unsupported action in this webview; keyboard shortcuts still work.
    }
  }

  return controller;
}
