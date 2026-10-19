import type { FileManagerClient } from '../../api/client/file-manager-client';
import { t } from '../../i18n';
import type { EntrySummary } from '../../models';
import type { OperationsController } from '../operations/operations-controller';
import { canWriteGps, GpsCache, scanGps } from './gps-scan';
import { type ImageMapState, INITIAL_IMAGE_MAP_STATE } from './image-map-state';

/** Where the images come from; re-run when the "include subfolders" option changes. */
export interface ImageMapSource {
  /** Whether the source is a folder, so that subfolders can be offered. */
  readonly folder: boolean;
  collect(
    recursive: boolean,
    signal: AbortSignal,
  ): Promise<{ readonly entries: readonly EntrySummary[]; readonly truncated: boolean }>;
}

export interface ImageMapControllerContext {
  getClient(): Pick<FileManagerClient, 'readImageGps' | 'listOperations'>;
  getOpsController(): Pick<OperationsController, 'editImageMetadata'>;
  getState(): ImageMapState;
  setState(state: ImageMapState): void;
  toast(message: string): void;
  redraw(): void;
}

export interface ImageMapController {
  open(source: ImageMapSource): Promise<void>;
  setRecursive(recursive: boolean): Promise<void>;
  cancel(): void;
  close(): void;
  beginPlacing(entry: EntrySummary): void;
  stopPlacing(): void;
  /** A marker was dragged, or the map was clicked while placing: ask for confirmation. */
  proposePlacement(entry: EntrySummary, latitude: number, longitude: number): void;
  confirmPlacement(): Promise<void>;
  cancelPlacement(): void;
}

const OPERATION_POLL_MS = 300;
const OPERATION_POLL_LIMIT = 100;

function message(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export function createImageMapController(context: ImageMapControllerContext): ImageMapController {
  const cache = new GpsCache();
  let source: ImageMapSource | undefined;
  let abort: AbortController | undefined;

  function patch(change: Partial<ImageMapState>): void {
    context.setState({ ...context.getState(), ...change });
    context.redraw();
  }

  async function run(recursive: boolean): Promise<void> {
    abort?.abort();
    const controller = new AbortController();
    abort = controller;
    const current = source;
    if (current === undefined) return;
    patch({
      open: true,
      scanning: true,
      done: 0,
      total: 0,
      recursive,
      foldersOffered: current.folder,
      failure: undefined,
      placing: undefined,
      pending: undefined,
    });
    try {
      const collected = await current.collect(recursive, controller.signal);
      controller.signal.throwIfAborted();
      patch({ total: collected.entries.length, truncated: collected.truncated });
      const result = await scanGps({
        client: context.getClient(),
        entries: collected.entries,
        cache,
        signal: controller.signal,
        onProgress: (done, total) => patch({ done, total }),
      });
      patch({
        scanning: false,
        located: result.located,
        unlocated: result.unlocated,
        unreadable: result.unreadable,
      });
    } catch (error) {
      if (controller.signal.aborted) {
        if (abort === controller) patch({ scanning: false });
        return;
      }
      patch({ scanning: false, failure: message(error) });
    }
  }

  async function waitForOperation(id: string): Promise<boolean> {
    for (let attempt = 0; attempt < OPERATION_POLL_LIMIT; attempt += 1) {
      const operation = (await context.getClient().listOperations()).find((item) => item.id === id);
      if (operation !== undefined) {
        if (operation.state === 'completed') return true;
        if (operation.state === 'failed' || operation.state === 'cancelled') return false;
      }
      await new Promise((resolve) => setTimeout(resolve, OPERATION_POLL_MS));
    }
    return false;
  }

  return {
    async open(next) {
      source = next;
      await run(false);
    },
    async setRecursive(recursive) {
      await run(recursive);
    },
    cancel() {
      abort?.abort();
      patch({ scanning: false });
    },
    close() {
      abort?.abort();
      source = undefined;
      context.setState(INITIAL_IMAGE_MAP_STATE);
      context.redraw();
    },
    beginPlacing(entry) {
      if (!canWriteGps(entry)) {
        context.toast(t('imageMap', 'cannotWrite', { name: entry.name }));
        return;
      }
      patch({ placing: entry });
    },
    stopPlacing() {
      patch({ placing: undefined });
    },
    proposePlacement(entry, latitude, longitude) {
      if (!canWriteGps(entry)) {
        context.toast(t('imageMap', 'cannotWrite', { name: entry.name }));
        // Re-publish the markers so a dragged one snaps back to its stored position.
        patch({ located: [...context.getState().located], placing: undefined });
        return;
      }
      patch({ pending: { entry, latitude, longitude }, placing: undefined });
    },
    cancelPlacement() {
      patch({ pending: undefined, located: [...context.getState().located] });
    },
    async confirmPlacement() {
      const pending = context.getState().pending;
      if (pending === undefined) return;
      patch({ pending: undefined });
      try {
        const operation = await context
          .getOpsController()
          .editImageMetadata([pending.entry.location], {
            gps: { latitude: pending.latitude, longitude: pending.longitude },
            backupOriginal: true,
          });
        if (!(await waitForOperation(operation.id))) {
          context.toast(t('imageMap', 'placementFailed'));
          patch({ located: [...context.getState().located] });
          return;
        }
        // Its modification time changed, so the cached result no longer applies.
        cache.forget(pending.entry);
        context.toast(t('imageMap', 'placed', { name: pending.entry.name }));
        await run(context.getState().recursive);
      } catch (error) {
        context.toast(t('imageMap', 'placementError', { message: message(error) }));
        patch({ located: [...context.getState().located] });
      }
    },
  };
}
