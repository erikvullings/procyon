import type { FileManagerClient } from '../../api/client/file-manager-client';
import { errorText, t } from '../../i18n';
import type { Location, MusicDuplicateGroup, MusicDuplicatePage } from '../../models';
import type { OperationsController } from '../operations/operations-controller';
import {
  INITIAL_MUSIC_DUPLICATES_STATE,
  type MusicDuplicatesState,
  suggestedRemovals,
  withoutRemoved,
  wouldRemoveEveryCopy,
} from './music-duplicates-state';

const PAGE_SIZE = 500;
const MAX_GROUPS = 5000;
const POLL_INTERVAL_MS = 700;

export interface MusicDuplicatesControllerContext {
  getClient(): Pick<
    FileManagerClient,
    'startMusicDuplicateScan' | 'getMusicDuplicateScan' | 'cancelMusicDuplicateScan'
  >;
  getOpsController(): Pick<OperationsController, 'trash'>;
  getState(): MusicDuplicatesState;
  setState(state: MusicDuplicatesState): void;
  toast(message: string): void;
  redraw(): void;
  /** Re-lists panes after files were moved to the Trash. */
  refreshPanes(): void;
}

export interface MusicDuplicatesController {
  start(roots: readonly Location[]): Promise<void>;
  cancel(): Promise<void>;
  close(): void;
  toggle(uri: string): void;
  selectSuggestedRemovals(): void;
  clearSelection(): void;
  removeSelected(): Promise<void>;
  isLastCopy(uri: string): boolean;
  dispose(): void;
}

function message(error: unknown): string {
  return errorText(error);
}

export function createMusicDuplicatesController(
  context: MusicDuplicatesControllerContext,
): MusicDuplicatesController {
  let timer: ReturnType<typeof setTimeout> | undefined;
  let generation = 0;

  function patch(change: Partial<MusicDuplicatesState>): void {
    context.setState({ ...context.getState(), ...change });
    context.redraw();
  }

  function stopPolling(): void {
    generation += 1;
    if (timer !== undefined) clearTimeout(timer);
    timer = undefined;
  }

  async function loadGroups(
    scanId: string,
    first: MusicDuplicatePage,
  ): Promise<MusicDuplicateGroup[]> {
    const groups = [...first.groups];
    let hasMore = first.hasMore;
    while (hasMore && groups.length < MAX_GROUPS) {
      const next = await context
        .getClient()
        .getMusicDuplicateScan(scanId, { offset: groups.length, limit: PAGE_SIZE });
      groups.push(...next.groups);
      hasMore = next.hasMore && next.groups.length > 0;
    }
    return groups;
  }

  async function poll(scanId: string, token: number): Promise<void> {
    try {
      const page = await context
        .getClient()
        .getMusicDuplicateScan(scanId, { offset: 0, limit: PAGE_SIZE });
      if (token !== generation) return;
      const groups = page.isComplete ? await loadGroups(scanId, page) : page.groups;
      if (token !== generation) return;
      patch({
        groups,
        phase: page.phase,
        filesFound: page.filesFound,
        filesProcessed: page.filesProcessed,
        unreadableTags: page.unreadableTags,
        isComplete: page.isComplete,
        isCancelled: page.isCancelled,
      });
      if (!page.isComplete) {
        timer = setTimeout(() => void poll(scanId, token), POLL_INTERVAL_MS);
      }
    } catch (error: unknown) {
      if (token === generation) patch({ error: message(error), isComplete: true });
    }
  }

  return {
    async start(roots) {
      stopPolling();
      const token = generation;
      context.setState({ ...INITIAL_MUSIC_DUPLICATES_STATE, scanId: 'starting' });
      context.redraw();
      try {
        const started = await context.getClient().startMusicDuplicateScan({ roots });
        if (token !== generation) return;
        patch({ scanId: started.scanId });
        await poll(started.scanId, token);
      } catch (error: unknown) {
        if (token !== generation) return;
        context.setState(INITIAL_MUSIC_DUPLICATES_STATE);
        context.toast(t('musicDuplicates', 'startFailed', { message: message(error) }));
        context.redraw();
      }
    },

    async cancel() {
      const { scanId } = context.getState();
      if (scanId === undefined || scanId === 'starting') return;
      try {
        await context.getClient().cancelMusicDuplicateScan(scanId);
      } catch (error: unknown) {
        patch({ error: message(error) });
      }
    },

    close() {
      stopPolling();
      const { scanId, isComplete } = context.getState();
      if (scanId !== undefined && scanId !== 'starting' && !isComplete) {
        void context
          .getClient()
          .cancelMusicDuplicateScan(scanId)
          .catch(() => undefined);
      }
      context.setState(INITIAL_MUSIC_DUPLICATES_STATE);
      context.redraw();
    },

    toggle(uri) {
      const state = context.getState();
      const selected = new Set(state.selectedUris);
      if (selected.has(uri)) selected.delete(uri);
      else if (!wouldRemoveEveryCopy(state.groups, selected, uri)) selected.add(uri);
      patch({ selectedUris: selected });
    },

    selectSuggestedRemovals() {
      patch({ selectedUris: suggestedRemovals(context.getState().groups) });
    },

    clearSelection() {
      patch({ selectedUris: new Set() });
    },

    async removeSelected() {
      const state = context.getState();
      if (state.selectedUris.size === 0 || state.removing) return;
      const locations = state.groups
        .flatMap((group) => group.files)
        .filter((file) => state.selectedUris.has(file.location.uri))
        .map((file) => file.location);
      patch({ removing: true, error: undefined });
      try {
        const operation = await context.getOpsController().trash(locations);
        if (operation === undefined) {
          patch({ removing: false });
          return;
        }
        const removed = new Set(state.selectedUris);
        patch({
          removing: false,
          groups: withoutRemoved(context.getState().groups, removed),
          selectedUris: new Set(),
        });
        context.toast(t('musicDuplicates', 'removedCount', locations.length));
        context.refreshPanes();
      } catch (error: unknown) {
        patch({
          removing: false,
          error: t('musicDuplicates', 'removeFailed', { message: message(error) }),
        });
      }
    },

    isLastCopy(uri) {
      const state = context.getState();
      return wouldRemoveEveryCopy(state.groups, state.selectedUris, uri);
    },

    dispose() {
      stopPolling();
    },
  };
}
