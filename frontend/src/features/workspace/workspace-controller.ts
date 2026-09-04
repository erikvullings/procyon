import type { FileManagerClient, NativeFileDrop } from '../../api/client/file-manager-client';
import { t } from '../../i18n';
import type {
  Connection,
  Location,
  SystemLocation,
  Volume,
  WorkspaceId,
  WorkspaceProjection,
  WorkspaceSummary,
} from '../../models';
import { loadConnections } from '../connections/connections-model';
import { FinderTagsLoader } from '../directory-table/finder-tags-loader';
import { NativeIconLoader } from '../directory-table/native-icon-loader';
import { ThumbnailLoader } from '../directory-table/thumbnail-loader';
import type { NavigationController } from '../navigation/navigation';
import type { SelectionPlatform } from '../selection/keybindings';
import { isWorkspaceRevisionConflict } from './dispatch-workspace-command';
import { firstAvailableWorkspaceId } from './workspace-manager';

export interface WorkspaceControllerContext {
  getWorkspace(): WorkspaceProjection | undefined;
  setWorkspace(ws: WorkspaceProjection | undefined): void;
  getWorkspaceError(): string | undefined;
  setWorkspaceError(msg?: string): void;
  getWorkspaceSummaries(): readonly WorkspaceSummary[];
  setWorkspaceSummaries(summaries: readonly WorkspaceSummary[]): void;
  getWorkspaceActionError(): string | undefined;
  setWorkspaceActionError(msg?: string): void;
  getWorkspaceRequest(): AbortController | undefined;
  setWorkspaceRequest(ac?: AbortController): void;
  getPlatform(): SelectionPlatform;
  setPlatform(p: SelectionPlatform): void;
  getNativeDragOutSupported(): boolean;
  setNativeDragOutSupported(v: boolean): void;
  getUnsubscribeNativeFileDrops(): (() => void) | undefined;
  setUnsubscribeNativeFileDrops(fn?: () => void): void;
  subscribeNativeFileDrops(callback: (drop: NativeFileDrop) => void): Promise<() => void>;
  getDraggedLocations(): readonly Location[];
  getNativeDragSourceInternal(): boolean;
  setNativeDragSourceInternal(v: boolean): void;
  setOpenTerminalSupported(v: boolean): void;
  setPlatformContextMenuSupported(v: boolean): void;
  setNativeIconLoader(loader?: NativeIconLoader): void;
  setThumbnailLoader(loader?: ThumbnailLoader): void;
  setFinderTagsLoader(loader?: FinderTagsLoader): void;
  getSystemLocations(): readonly SystemLocation[];
  setSystemLocations(locs: readonly SystemLocation[]): void;
  setSystemLocationsError(msg?: string): void;
  getVolumes(): readonly Volume[];
  setVolumes(volumes: readonly Volume[]): void;
  setVolumesError(msg?: string): void;
  setHomeDirectory(path: string | undefined): void;
  getConnections(): readonly Connection[];
  setConnections(conns: readonly Connection[]): void;
  setDraggedLocations(locs: readonly Location[]): void;
  getNativeDropInProgress(): boolean;
  setNativeDropInProgress(v: boolean): void;
  setClipboardMessage(msg?: string): void;
  getNavigation(): NavigationController;
  getFlushPendingLayoutUpdate(): (() => void) | undefined;
  redraw(): void;
  releaseWorkspaceTabState(outgoing: WorkspaceProjection): void;
  loadPanesActiveFirst(ws: WorkspaceProjection): void;
  syncWorkspaceViewSettings?(): void;
}

export interface WorkspaceController {
  activateWorkspace(loaded: WorkspaceProjection): void;
  recoverActiveWorkspace(summaries: readonly WorkspaceSummary[]): Promise<void>;
  loadWorkspace(): Promise<void>;
  loadSystemLocations(signal?: AbortSignal): Promise<void>;
  loadVolumes(signal?: AbortSignal): Promise<void>;
  loadHomeDirectory(signal?: AbortSignal): Promise<void>;
  switchWorkspace(workspaceId: WorkspaceId): Promise<void>;
  refreshWorkspaceSummaries(): void;
  revisionForWorkspace(workspaceId: WorkspaceId): number;
  createWorkspaceAction(): void;
  renameWorkspaceAction(workspaceId: WorkspaceId, name: string): void;
  deleteWorkspaceAction(workspaceId: WorkspaceId): void;
}

function workspaceErrorMessage(error: unknown, fallback: string): string {
  return error instanceof Error ? error.message : fallback;
}

function locationKey(location: Location): string {
  try {
    const url = new URL(location.uri);
    const pathSegments = url.pathname
      .split('/')
      .map((segment) => decodeURIComponent(segment).normalize('NFC'));
    return JSON.stringify([
      location.providerId,
      url.protocol,
      url.hostname === 'localhost' ? '' : url.host,
      pathSegments,
      url.search,
      url.hash,
    ]);
  } catch {
    return `${location.providerId} ${location.uri.normalize('NFC')}`;
  }
}

export function locationsMatch(a: readonly Location[], b: readonly Location[]): boolean {
  if (a.length !== b.length) return false;
  const remaining = new Set(a.map(locationKey));
  return b.every((location) => remaining.delete(locationKey(location)));
}

export function createWorkspaceController(
  client: FileManagerClient,
  context: WorkspaceControllerContext,
): WorkspaceController {
  function activateWorkspace(loaded: WorkspaceProjection): void {
    context.getFlushPendingLayoutUpdate()?.();
    const current = context.getWorkspace();
    if (current !== undefined) {
      context.releaseWorkspaceTabState(current);
    }
    context.setWorkspace(loaded);
    context.setWorkspaceError(undefined);
    context.loadPanesActiveFirst(loaded);
    context.syncWorkspaceViewSettings?.();
  }

  async function openOrCreateDefaultWorkspace(
    signal?: AbortSignal,
  ): Promise<{ loaded: WorkspaceProjection; summaries: readonly WorkspaceSummary[] }> {
    // Goes through the backend's startup lifecycle (spec §5.3.7) rather than picking
    // `listWorkspaces()[0]` (an unsorted filesystem-order listing): this reliably reopens the
    // workspace that was actually last active, not just an arbitrary one when several exist.
    // A window opened for a specific workspace (task 0143 sub-task (b), see
    // `open_workspace_window` on the Tauri side) carries that workspace's id in the URL so this
    // window starts on it explicitly instead of racing every other open window for last-active.
    const requestedWorkspaceId = new URLSearchParams(window.location.search).get('workspaceId');
    const loaded = await client.startWorkspace(requestedWorkspaceId ?? undefined, signal);
    const summaries = await client.listWorkspaces(signal);
    return { loaded, summaries };
  }

  async function recoverActiveWorkspace(summaries: readonly WorkspaceSummary[]): Promise<void> {
    const nextId = firstAvailableWorkspaceId(summaries);
    if (nextId === undefined) {
      const created = await client.createWorkspace({ name: 'Default' });
      activateWorkspace(created);
      context.setWorkspaceSummaries(await client.listWorkspaces());
      return;
    }
    await switchWorkspace(nextId);
  }

  async function loadSystemLocations(signal?: AbortSignal): Promise<void> {
    try {
      context.setSystemLocations(await client.getSystemLocations(signal));
      context.setSystemLocationsError(undefined);
    } catch {
      context.setSystemLocations([]);
      context.setSystemLocationsError(t('workspace', 'unableToDiscoverCloudLocations'));
    }
    context.redraw();
  }

  async function loadVolumes(signal?: AbortSignal): Promise<void> {
    try {
      context.setVolumes(await client.getVolumes(signal));
      context.setVolumesError(undefined);
    } catch {
      context.setVolumes([]);
      context.setVolumesError(t('workspace', 'unableToDiscoverVolumes'));
    }
    context.redraw();
  }

  async function loadHomeDirectory(signal?: AbortSignal): Promise<void> {
    try {
      context.setHomeDirectory(await client.getHomeDirectory(signal));
    } catch {
      context.setHomeDirectory(undefined);
    }
  }

  async function loadConnectionsList(signal?: AbortSignal): Promise<void> {
    try {
      context.setConnections(await loadConnections(client, signal));
    } catch {
      context.setConnections([]);
    }
    context.redraw();
  }

  async function loadWorkspace(): Promise<void> {
    const request = new AbortController();
    context.setWorkspaceRequest(request);
    try {
      const capabilities = await client.getRuntimeCapabilities(request.signal);
      await loadSystemLocations(request.signal);
      await loadVolumes(request.signal);
      await loadHomeDirectory(request.signal);
      await loadConnectionsList(request.signal);
      context.setPlatform(capabilities.platform);
      context.setNativeDragOutSupported(capabilities.nativeDragOut);
      if (capabilities.nativeDragOut && context.getUnsubscribeNativeFileDrops() === undefined) {
        const unsub = await context.subscribeNativeFileDrops((drop) => {
          // A drag started in this window (dragging a selection between panes/tabs) still goes
          // through the OS-level native drag session when `nativeDragOut` is on, so it round-trips
          // through this same "native file drop" handler. Only a drop whose locations don't match
          // what we ourselves started dragging is a genuine external drop (from Finder/Explorer),
          // which forces `copy`; an in-app drag defaults to `move` like any other drag here.
          const previousLocations = context.getDraggedLocations();
          const wasInternalOrigin = context.getNativeDragSourceInternal();
          context.setNativeDragSourceInternal(false);
          const isInternalDrop =
            wasInternalOrigin && locationsMatch(previousLocations, drop.locations);
          context.setDraggedLocations(isInternalDrop ? previousLocations : drop.locations);
          const scale = window.devicePixelRatio || 1;
          const hit = document.elementFromPoint(drop.position.x / scale, drop.position.y / scale);
          const target = hit?.closest<HTMLElement>(
            '.fm-directory-row, .fm-directory-viewport, .fm-pane-tab',
          );
          if (target === undefined || target === null) {
            context.setClipboardMessage(t('workspace', 'dropFilesOntoPaneOrTab'));
            context.redraw();
            return;
          }
          context.setNativeDropInProgress(!isInternalDrop);
          try {
            target.dispatchEvent(new Event('drop', { bubbles: true, cancelable: true }));
          } finally {
            context.setNativeDropInProgress(false);
          }
        });
        context.setUnsubscribeNativeFileDrops(unsub);
      }
      context.setOpenTerminalSupported(capabilities.openTerminal);
      context.setPlatformContextMenuSupported(capabilities.platformContextMenu);
      context.setNativeIconLoader(
        capabilities.nativeFileIcons ? new NativeIconLoader(client) : undefined,
      );
      // Not capability-gated (task 0134): the backend's pure-Rust thumbnail
      // pipeline works on every runtime/platform, unlike native OS icons.
      // Unsupported formats/files simply fail per-request and fall back to
      // the themed icon, exactly like a `nativeFileIcons: false` host does.
      context.setThumbnailLoader(new ThumbnailLoader(client));
      context.setFinderTagsLoader(
        capabilities.finderTags ? new FinderTagsLoader(client) : undefined,
      );
      const { loaded, summaries } = await openOrCreateDefaultWorkspace(request.signal);
      activateWorkspace(loaded);
      context.setWorkspaceSummaries(summaries);
    } catch (error: unknown) {
      if (request.signal.aborted) return;
      context.setWorkspaceError(
        workspaceErrorMessage(error, t('workspace', 'unableToLoadWorkspace')),
      );
    }
    context.redraw();
  }

  async function switchWorkspace(workspaceId: WorkspaceId): Promise<void> {
    if (context.getWorkspace()?.id === workspaceId) return;
    context.getWorkspaceRequest()?.abort();
    const request = new AbortController();
    context.setWorkspaceRequest(request);
    context.setWorkspaceActionError(undefined);
    try {
      const loaded = await client.openWorkspace(workspaceId, request.signal);
      activateWorkspace(loaded);
      context.setWorkspaceSummaries(await client.listWorkspaces(request.signal));
    } catch (error: unknown) {
      if (request.signal.aborted) return;
      context.setWorkspaceActionError(
        workspaceErrorMessage(error, t('workspace', 'unableToSwitchWorkspace')),
      );
    }
    context.redraw();
  }

  function refreshWorkspaceSummaries(): void {
    void client
      .listWorkspaces()
      .then((summaries) => {
        context.setWorkspaceSummaries(summaries);
        context.redraw();
      })
      .catch(() => undefined);
  }

  function revisionForWorkspace(workspaceId: WorkspaceId): number {
    const ws = context.getWorkspace();
    if (ws?.id === workspaceId) return ws.revision;
    return (
      context.getWorkspaceSummaries().find((summary) => summary.id === workspaceId)?.revision ?? 0
    );
  }

  function createWorkspaceAction(): void {
    context.setWorkspaceActionError(undefined);
    void client
      .createWorkspace({})
      .then(async (created) => {
        activateWorkspace(created);
        context.setWorkspaceSummaries(await client.listWorkspaces());
        context.redraw();
      })
      .catch((error: unknown) => {
        context.setWorkspaceActionError(
          workspaceErrorMessage(error, t('workspace', 'unableToCreateWorkspace')),
        );
        context.redraw();
      });
  }

  function renameWorkspaceAction(workspaceId: WorkspaceId, name: string): void {
    context.setWorkspaceActionError(undefined);
    void client
      .renameWorkspace(workspaceId, name, revisionForWorkspace(workspaceId))
      .then(async (updated) => {
        if (context.getWorkspace()?.id === workspaceId) context.setWorkspace(updated);
        context.setWorkspaceSummaries(await client.listWorkspaces());
        context.redraw();
      })
      .catch(async (error: unknown) => {
        if (isWorkspaceRevisionConflict(error)) {
          context.setWorkspaceSummaries(
            await client.listWorkspaces().catch(() => context.getWorkspaceSummaries()),
          );
          context.setWorkspaceActionError(t('workspace', 'renameConflict'));
        } else {
          context.setWorkspaceActionError(
            workspaceErrorMessage(error, t('workspace', 'unableToRenameWorkspace')),
          );
        }
        context.redraw();
      });
  }

  function deleteWorkspaceAction(workspaceId: WorkspaceId): void {
    context.setWorkspaceActionError(undefined);
    const wasActive = context.getWorkspace()?.id === workspaceId;
    void client
      .deleteWorkspace(workspaceId, revisionForWorkspace(workspaceId))
      .then(async () => {
        const summaries = await client.listWorkspaces();
        context.setWorkspaceSummaries(summaries);
        if (wasActive) await recoverActiveWorkspace(summaries);
        context.redraw();
      })
      .catch((error: unknown) => {
        context.setWorkspaceActionError(
          isWorkspaceRevisionConflict(error)
            ? t('workspace', 'deleteConflict')
            : workspaceErrorMessage(error, t('workspace', 'unableToDeleteWorkspace')),
        );
        context.redraw();
      });
  }

  return {
    activateWorkspace,
    recoverActiveWorkspace,
    loadWorkspace,
    loadSystemLocations,
    loadHomeDirectory,
    loadVolumes,
    switchWorkspace,
    refreshWorkspaceSummaries,
    revisionForWorkspace,
    createWorkspaceAction,
    renameWorkspaceAction,
    deleteWorkspaceAction,
  };
}
