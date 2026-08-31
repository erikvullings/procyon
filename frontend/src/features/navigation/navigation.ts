import type { FileManagerClient } from '../../api/client/file-manager-client';
import type {
  DirectorySnapshot,
  EntrySummary,
  ListDirectoryRequest,
  LoadingState,
  Location,
  PaneId,
  TabId,
  TabProjection,
  VolumeCapacity,
  WorkspaceCommand,
  WorkspaceProjection,
} from '../../models';
import { dispatchWorkspaceCommand } from '../workspace/dispatch-workspace-command';

/** Client surface required by directory navigation. */
export type NavigationClient = Pick<
  FileManagerClient,
  'dispatchWorkspaceCommand' | 'getWorkspace' | 'listDirectory' | 'navigatePane'
>;

/** Renderable directory state for one pane, including paging information. */
export interface PaneDirectoryView {
  readonly state: LoadingState;
  readonly entries: readonly EntrySummary[];
  readonly location?: Location;
  readonly writable?: boolean;
  readonly requestId?: string;
  readonly revision?: number;
  readonly hasMore: boolean;
  readonly continuationToken?: string;
  /** Total entries in the directory, known from the first page's response even before all pages load. */
  readonly totalKnownEntries?: number;
  /** Combined byte size of every file/symlink entry, known from the first page's response. */
  readonly totalKnownSize?: number;
  /** Number of file/symlink entries (directories excluded), known from the first page's response. */
  readonly totalKnownFileCount?: number;
  /** Backing volume's total/available capacity, when known (task 0096). */
  readonly volumeCapacity?: VolumeCapacity;
}

/** Integration callbacks kept outside the navigation module. */
export interface NavigationControllerOptions {
  readonly client: NavigationClient;
  readonly getWorkspace: () => WorkspaceProjection | undefined;
  readonly replaceWorkspace: (workspace: WorkspaceProjection) => void;
  /** Called after a location has been successfully opened through navigation history. */
  readonly onLocationVisited?: (workspaceId: string, location: Location) => void;
  /** Called when a requested location cannot be opened. */
  readonly onLocationUnavailable?: (workspaceId: string, location: Location) => void;
  /** Prompts for a session-only archive password; resolves false when cancelled. */
  readonly requestArchivePassword?: (location: Location, invalid: boolean) => Promise<boolean>;
  /**
   * Whether the git-status column is currently visible. The backend never computes git
   * working-tree status (a `git2` walk that can be expensive on a large repository) when this
   * is `false`, so most listings pay nothing for it at all. Omit to always send `false`
   * (the column's own default).
   */
  readonly getShowGitStatusColumn?: () => boolean;
  /**
   * `preferredCursorName`, when set, is the entry name the pane's cursor
   * should land on instead of the listing's first entry (e.g. the child
   * directory just navigated away from via `parent()`).
   */
  readonly updatePane: (
    paneId: PaneId,
    tabId: TabId,
    view: PaneDirectoryView,
    preferredCursorName?: string,
  ) => void;
}

/** Public navigation operations consumed by pane and workspace input handlers. */
export interface NavigationController {
  /**
   * Reloads the pane's active tab. `background: true` marks the call as an opportunistic
   * refresh (e.g. triggered by a filesystem-watch delta) rather than a user-requested reload:
   * it's skipped entirely while an explicit `navigate()`/`parent()`/`back()`/`forward()` is
   * still in flight for the same tab, so it can never silently discard that navigation's own
   * snapshot fetch (see fm-search results not appearing after navigating to `search://`).
   */
  load(paneId: PaneId, options?: { readonly background?: boolean }): Promise<void>;
  navigate(paneId: PaneId, location: Location, preferredCursorName?: string): Promise<void>;
  parent(paneId: PaneId): Promise<void>;
  back(paneId: PaneId): Promise<void>;
  forward(paneId: PaneId): Promise<void>;
  retry(paneId: PaneId): Promise<void>;
  loadNextPage(paneId: PaneId): Promise<void>;
  /** Loads every remaining page for the pane's current directory (e.g. before jumping to the last entry). */
  loadAllPages(paneId: PaneId): Promise<void>;
  /** Cancels a specific tab's in-flight request, e.g. because it just became hidden. */
  abort(paneId: PaneId, tabId: TabId): void;
  /** Re-keys a tab's cached directory view after it moves between panes. */
  moveTab(sourcePaneId: PaneId, targetPaneId: PaneId, tabId: TabId): void;
  dispose(): void;
}

/** Distinguishes an explicit navigation (push/back/forward/parent) from a plain reload/refresh. */
type RequestKind = 'navigate' | 'load';

interface ActiveRequest {
  readonly id: string;
  readonly controller: AbortController;
  readonly kind: RequestKind;
}

/** Tauri's `invoke` rejects with the serialized `ApplicationErrorDto` rather than an `Error`,
 * so a plain object carrying a `message` must not collapse into the generic fallback. */
function errorMessage(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === 'object' && error !== null) {
    const { message } = error as { readonly message?: unknown };
    if (typeof message === 'string' && message.length > 0) return message;
  }
  return 'Unable to load directory';
}

function applicationErrorCode(error: unknown): string | undefined {
  if (typeof error !== 'object' || error === null) return undefined;
  const code = (error as { readonly code?: unknown }).code;
  return typeof code === 'string' ? code : undefined;
}

function isRetryableNavigationError(error: unknown): boolean {
  const code = applicationErrorCode(error);
  return (
    code === 'platformOperationFailed' || code === 'providerUnavailable' || code === 'internal'
  );
}

/**
 * Consecutive background-refresh failures a tab absorbs silently (e.g. an SSH connection
 * dropping over VPN while the pane is idle) before the error is actually shown to the user.
 * Each attempt is retried with `BACKGROUND_RETRY_BACKOFF_MS` backoff; a success at any point
 * resets the count without disturbing the currently-published view.
 */
const BACKGROUND_RETRY_THRESHOLD = 3;
const BACKGROUND_RETRY_BACKOFF_MS = [500, 1500, 3000];

function activeTab(workspace: WorkspaceProjection, paneId: PaneId) {
  const pane = workspace.panesById[paneId];
  return pane?.tabsById[pane.activeTabId];
}

/** Returns a provider-preserving lexical parent; roots map to themselves. */
export function parentLocation(location: Location): Location {
  try {
    if (location.providerId === 'archive' && location.uri.startsWith('archive://')) {
      const remainder = location.uri.slice('archive://'.length);
      const separator = remainder.indexOf('!');
      if (separator >= 0) {
        const outer = remainder.slice(0, separator);
        const inner = remainder.slice(separator + 1).replace(/^\/+|\/+$/g, '');
        if (inner.length === 0) {
          return parentLocation({ providerId: 'local', uri: `file://${outer}` });
        }
        const finalSeparator = inner.lastIndexOf('/');
        const parentInner = finalSeparator < 0 ? '' : inner.slice(0, finalSeparator);
        return {
          providerId: 'archive',
          uri: `archive://${outer}!/${parentInner}`,
        };
      }
    }
    const url = new URL(location.uri);
    const path = url.pathname;
    if (path === '/' || path.length === 0) {
      return location;
    }
    const trimmed = path.replace(/\/+$/, '');
    const finalSeparator = trimmed.lastIndexOf('/');
    url.pathname = finalSeparator <= 0 ? '/' : trimmed.slice(0, finalSeparator);
    return { ...location, uri: url.toString() };
  } catch {
    return location;
  }
}

/** Returns the final path segment (decoded) of a location, e.g. for cursor restoration after `..`. */
function lastPathSegment(location: Location): string | undefined {
  try {
    if (location.providerId === 'archive' && location.uri.startsWith('archive://')) {
      const [outer, rawInner = ''] = location.uri.slice('archive://'.length).split('!', 2);
      const inner = rawInner.replace(/^\/+|\/+$/g, '');
      if (inner.length > 0) {
        return decodeURIComponent(inner.slice(inner.lastIndexOf('/') + 1));
      }
      if (outer !== undefined) {
        return decodeURIComponent(outer.slice(outer.lastIndexOf('/') + 1));
      }
    }
    const path = new URL(location.uri).pathname.replace(/\/+$/, '');
    const finalSeparator = path.lastIndexOf('/');
    const segment = finalSeparator < 0 ? path : path.slice(finalSeparator + 1);
    return segment.length === 0 ? undefined : decodeURIComponent(segment);
  } catch {
    return undefined;
  }
}

/** Isolates cancellable requests and cached views per tab, not merely per pane. */
function tabKey(paneId: PaneId, tabId: TabId): string {
  return `${paneId}:${tabId}`;
}

/** Coordinates workspace history and cancellable directory requests per (pane, tab). */
export function createNavigationController(
  options: NavigationControllerOptions,
): NavigationController {
  const activeRequests = new Map<string, ActiveRequest>();
  const paneViews = new Map<string, PaneDirectoryView>();
  // Dedupes concurrent `loadNextPage` calls for the same tab (e.g. repeated `onEndReached`
  // firing while scrolling) so a later call reuses the in-flight fetch instead of aborting it
  // via `begin()` and restarting from scratch — otherwise a fast scroll can cancel-and-restart
  // the fetch forever, so it never completes.
  const pendingNextPage = new Map<string, Promise<void>>();
  // Consecutive background-load failures per tab, and any pending silent-retry timer for it.
  // Cleared on success or on `dispose()`; see `BACKGROUND_RETRY_THRESHOLD`.
  const backgroundFailureCounts = new Map<string, number>();
  const backgroundRetryTimers = new Map<string, ReturnType<typeof setTimeout>>();

  function clearBackgroundRetry(key: string): void {
    const timer = backgroundRetryTimers.get(key);
    if (timer !== undefined) {
      clearTimeout(timer);
      backgroundRetryTimers.delete(key);
    }
  }

  async function withArchiveCredential<T>(
    location: Location,
    operation: () => Promise<T>,
  ): Promise<T> {
    for (;;) {
      try {
        return await operation();
      } catch (error: unknown) {
        const code = applicationErrorCode(error);
        if (
          location.providerId !== 'archive' ||
          options.requestArchivePassword === undefined ||
          (code !== 'credentialRequired' && code !== 'invalidCredential') ||
          !(await options.requestArchivePassword(location, code === 'invalidCredential'))
        ) {
          throw error;
        }
      }
    }
  }

  function begin(paneId: PaneId, tabId: TabId, kind: RequestKind): ActiveRequest {
    const key = tabKey(paneId, tabId);
    activeRequests.get(key)?.controller.abort();
    clearBackgroundRetry(key);
    const request = {
      id: crypto.randomUUID(),
      controller: new AbortController(),
      kind,
    };
    activeRequests.set(key, request);
    return request;
  }

  function isCurrent(paneId: PaneId, tabId: TabId, request: ActiveRequest): boolean {
    const key = tabKey(paneId, tabId);
    return activeRequests.get(key)?.id === request.id && !request.controller.signal.aborted;
  }

  function finish(paneId: PaneId, tabId: TabId, request: ActiveRequest): void {
    const key = tabKey(paneId, tabId);
    if (activeRequests.get(key)?.id === request.id) activeRequests.delete(key);
  }

  function publish(
    paneId: PaneId,
    tabId: TabId,
    view: PaneDirectoryView,
    preferredCursorName?: string,
  ): void {
    paneViews.set(tabKey(paneId, tabId), view);
    options.updatePane(paneId, tabId, view, preferredCursorName);
  }

  function loadingView(
    paneId: PaneId,
    tabId: TabId,
    request: ActiveRequest,
    fallbackLocation: Location,
  ): PaneDirectoryView {
    const current = paneViews.get(tabKey(paneId, tabId));
    return {
      state: { type: 'loading' },
      entries: current?.entries ?? [],
      location: current?.location ?? fallbackLocation,
      requestId: request.id,
      hasMore: false,
    };
  }

  function requestFor(
    workspace: WorkspaceProjection,
    paneId: PaneId,
    requestId: string,
    location: Location,
    tab: TabProjection | undefined,
    continuationToken?: string,
  ): ListDirectoryRequest {
    return {
      workspaceId: workspace.id,
      paneId,
      requestId,
      location,
      ...(continuationToken === undefined ? {} : { continuationToken }),
      ...viewOptionsFor(tab),
    };
  }

  /**
   * The subset of a tab's view (`sort`/`showHidden`/`foldersFirst`/`showGitStatus`) that should
   * be carried over into a fresh listing/navigation request, so pushing a new location - e.g.
   * via a favourite, breadcrumb, or opening a subfolder - doesn't silently reset the tab back to
   * backend default view settings.
   */
  function viewOptionsFor(
    tab: TabProjection | undefined,
  ): Pick<ListDirectoryRequest, 'sort' | 'showHidden' | 'foldersFirst' | 'showGitStatus'> {
    return {
      ...(tab?.view.sort === undefined ? {} : { sort: tab.view.sort }),
      ...(tab === undefined
        ? {}
        : { showHidden: tab.view.showHidden, foldersFirst: tab.view.foldersFirst }),
      showGitStatus: options.getShowGitStatusColumn?.() ?? false,
    };
  }

  function viewFromSnapshot(
    snapshot: DirectorySnapshot,
    entries: readonly EntrySummary[] = snapshot.entries,
  ): PaneDirectoryView {
    return {
      state: snapshot.loadingState,
      entries,
      location: snapshot.location,
      writable: snapshot.writable,
      requestId: snapshot.requestId,
      revision: snapshot.revision,
      hasMore: snapshot.hasMore,
      ...(snapshot.continuationToken === undefined
        ? {}
        : { continuationToken: snapshot.continuationToken }),
      ...(snapshot.totalKnownEntries === undefined
        ? {}
        : { totalKnownEntries: snapshot.totalKnownEntries }),
      ...(snapshot.totalKnownSize === undefined ? {} : { totalKnownSize: snapshot.totalKnownSize }),
      ...(snapshot.totalKnownFileCount === undefined
        ? {}
        : { totalKnownFileCount: snapshot.totalKnownFileCount }),
      ...(snapshot.volumeCapacity === undefined ? {} : { volumeCapacity: snapshot.volumeCapacity }),
    };
  }

  async function hydrateBackgroundSnapshot(
    workspace: WorkspaceProjection,
    paneId: PaneId,
    tab: TabProjection,
    request: ActiveRequest,
    firstSnapshot: DirectorySnapshot,
    minEntries: number,
  ): Promise<DirectorySnapshot> {
    let mergedEntries = [...firstSnapshot.entries];
    let hydratedSnapshot = firstSnapshot;
    let continuationToken = firstSnapshot.continuationToken;
    while (
      isCurrent(paneId, tab.id, request) &&
      hydratedSnapshot.hasMore &&
      continuationToken !== undefined &&
      mergedEntries.length < minEntries
    ) {
      const nextSnapshot = await options.client.listDirectory(
        requestFor(
          workspace,
          paneId,
          request.id,
          hydratedSnapshot.location,
          tab,
          continuationToken,
        ),
        request.controller.signal,
      );
      mergedEntries = [...mergedEntries, ...nextSnapshot.entries];
      hydratedSnapshot = nextSnapshot;
      continuationToken = nextSnapshot.continuationToken;
    }
    if (mergedEntries.length === firstSnapshot.entries.length) {
      return firstSnapshot;
    }
    return { ...hydratedSnapshot, entries: mergedEntries };
  }

  async function load(
    paneId: PaneId,
    loadOptions?: { readonly background?: boolean },
  ): Promise<void> {
    const workspace = options.getWorkspace();
    const tab = workspace === undefined ? undefined : activeTab(workspace, paneId);
    if (workspace === undefined || tab === undefined) {
      return;
    }
    const current = paneViews.get(tabKey(paneId, tab.id));
    if (loadOptions?.background) {
      const inFlight = activeRequests.get(tabKey(paneId, tab.id));
      // A background refresh (filesystem-watch delta) must never preempt an explicit navigation
      // that's already in flight for this tab: `begin()` aborts unconditionally, and doing so here
      // would silently discard the navigation's own snapshot fetch with no error and no retry -
      // e.g. a `directory.delta` for the pane's old location racing a search-results `navigate()`.
      if (
        inFlight !== undefined &&
        inFlight.kind === 'navigate' &&
        !inFlight.controller.signal.aborted
      ) {
        return;
      }
    }
    const request = begin(paneId, tab.id, 'load');
    if (!loadOptions?.background) {
      publish(paneId, tab.id, loadingView(paneId, tab.id, request, tab.location));
    }
    try {
      let snapshot = await withArchiveCredential(tab.location, () =>
        options.client.listDirectory(
          requestFor(workspace, paneId, request.id, tab.location, tab),
          request.controller.signal,
        ),
      );
      if (
        loadOptions?.background &&
        current !== undefined &&
        current.entries.length > snapshot.entries.length &&
        snapshot.hasMore
      ) {
        snapshot = await hydrateBackgroundSnapshot(
          workspace,
          paneId,
          tab,
          request,
          snapshot,
          current.entries.length,
        );
      }
      if (isCurrent(paneId, tab.id, request) && snapshot.requestId === request.id) {
        publish(paneId, tab.id, viewFromSnapshot(snapshot));
      }
      const key = tabKey(paneId, tab.id);
      backgroundFailureCounts.delete(key);
      clearBackgroundRetry(key);
    } catch (error: unknown) {
      if (!isCurrent(paneId, tab.id, request)) {
        return;
      }
      if (loadOptions?.background) {
        const key = tabKey(paneId, tab.id);
        const failures = (backgroundFailureCounts.get(key) ?? 0) + 1;
        backgroundFailureCounts.set(key, failures);
        // A connection drop while the pane is merely idle (no user activity) shouldn't be
        // obvious: retry silently in the background a few times with backoff, and only fall
        // through to the visible error state once it's genuinely failed repeatedly in a row.
        if (failures <= BACKGROUND_RETRY_THRESHOLD) {
          clearBackgroundRetry(key);
          const delay =
            BACKGROUND_RETRY_BACKOFF_MS[
              Math.min(failures - 1, BACKGROUND_RETRY_BACKOFF_MS.length - 1)
            ];
          const timer = setTimeout(() => {
            backgroundRetryTimers.delete(key);
            void load(paneId, { background: true });
          }, delay);
          backgroundRetryTimers.set(key, timer);
          return;
        }
      }
      publish(paneId, tab.id, {
        state: { type: 'error', message: errorMessage(error) },
        entries: [],
        location: tab.location,
        requestId: request.id,
        hasMore: false,
      });
    } finally {
      finish(paneId, tab.id, request);
    }
  }

  async function navigateHistory(
    paneId: PaneId,
    navigationMode: 'push' | 'back' | 'forward',
    location?: Location,
    preferredCursorName?: string,
  ): Promise<void> {
    const workspace = options.getWorkspace();
    const pane = workspace?.panesById[paneId];
    const tab = pane?.tabsById[pane.activeTabId];
    if (
      workspace === undefined ||
      pane === undefined ||
      tab === undefined ||
      (navigationMode === 'back' && !tab.canNavigateBack) ||
      (navigationMode === 'forward' && !tab.canNavigateForward)
    ) {
      return;
    }
    const request = begin(paneId, tab.id, 'navigate');
    publish(paneId, tab.id, loadingView(paneId, tab.id, request, location ?? tab.location));
    const command: WorkspaceCommand = {
      type: 'navigateTab',
      workspaceId: workspace.id,
      paneId,
      tabId: tab.id,
      navigationMode,
      expectedRevision: workspace.revision,
      ...(location === undefined ? {} : { location }),
    };
    const navigatePane = async (
      currentWorkspaceId: string,
      currentPaneId: PaneId,
      currentRequestId: string,
      currentLocation: Location,
      currentTab: TabProjection,
      currentSignal: AbortSignal,
    ): Promise<DirectorySnapshot> => {
      const payload = {
        workspaceId: currentWorkspaceId,
        paneId: currentPaneId,
        requestId: currentRequestId,
        location: currentLocation,
        ...viewOptionsFor(currentTab),
      };
      try {
        return await options.client.navigatePane(payload, currentSignal);
      } catch (error: unknown) {
        if (currentSignal.aborted || !isRetryableNavigationError(error)) {
          throw error;
        }
        return options.client.navigatePane(payload, currentSignal);
      }
    };
    try {
      // Explicit destinations can be validated without mutating workspace history. This keeps a
      // failed archive open (for example an unsupported RAR-backed CBR) from replacing the tab's
      // last usable location and poisoning retry, reload, and breadcrumb navigation.
      const pendingSnapshot =
        location === undefined
          ? undefined
          : await withArchiveCredential(location, () =>
              navigatePane(
                workspace.id,
                paneId,
                request.id,
                location,
                tab,
                request.controller.signal,
              ),
            );
      if (!isCurrent(paneId, tab.id, request)) {
        return;
      }
      // Goes through the resilient wrapper (not the raw client call) so a revision conflict
      // still resyncs the local workspace projection via `options.replaceWorkspace` even though
      // push/back/forward navigation isn't safe to silently retry — otherwise the local revision
      // is left permanently stale and every subsequent navigation command in the workspace (any
      // pane) keeps failing with the same conflict until something else happens to resync it.
      const updated = await dispatchWorkspaceCommand(
        options.client,
        command,
        options.replaceWorkspace,
        request.controller.signal,
      );
      if (!isCurrent(paneId, tab.id, request)) {
        return;
      }
      const updatedTab = activeTab(updated, paneId);
      if (updatedTab === undefined) {
        return;
      }
      const snapshot =
        pendingSnapshot ??
        (await withArchiveCredential(updatedTab.location, () =>
          navigatePane(
            updated.id,
            paneId,
            request.id,
            updatedTab.location,
            updatedTab,
            request.controller.signal,
          ),
        ));
      if (isCurrent(paneId, tab.id, request) && snapshot.requestId === request.id) {
        publish(paneId, tab.id, viewFromSnapshot(snapshot), preferredCursorName);
        options.onLocationVisited?.(updated.id, updatedTab.location);
      }
    } catch (error: unknown) {
      if (location !== undefined) {
        options.onLocationUnavailable?.(workspace.id, location);
      }
      if (isCurrent(paneId, tab.id, request)) {
        const currentTab = options.getWorkspace();
        publish(paneId, tab.id, {
          state: { type: 'error', message: errorMessage(error) },
          entries: [],
          location:
            (currentTab === undefined ? undefined : activeTab(currentTab, paneId)?.location) ??
            tab.location,
          requestId: request.id,
          hasMore: false,
        });
      }
    } finally {
      finish(paneId, tab.id, request);
    }
  }

  // `loadNextPageImpl`/`loadNextPage`/`loadAllPages` all take an explicit `tabId` pinned by
  // their caller (defaulting to the pane's active tab at the moment of the call) rather than
  // re-resolving `pane.activeTabId` on every invocation. Otherwise, if the user switches tabs
  // while `loadAllPages`'s loop is still awaiting a page, the next iteration would silently
  // start fetching pages for whichever tab is now active instead of stopping for the original
  // (now-hidden) tab, corrupting both tabs' entries.
  async function loadNextPageImpl(paneId: PaneId, tabId: TabId): Promise<void> {
    const workspace = options.getWorkspace();
    const tab = workspace?.panesById[paneId]?.tabsById[tabId];
    const current = paneViews.get(tabKey(paneId, tabId));
    if (
      workspace === undefined ||
      tab === undefined ||
      current?.location === undefined ||
      !current.hasMore ||
      current.continuationToken === undefined
    ) {
      return;
    }
    const request = begin(paneId, tabId, 'load');
    try {
      const snapshot = await options.client.listDirectory(
        requestFor(workspace, paneId, request.id, current.location, tab, current.continuationToken),
        request.controller.signal,
      );
      if (isCurrent(paneId, tabId, request) && snapshot.requestId === request.id) {
        publish(
          paneId,
          tabId,
          viewFromSnapshot(snapshot, [...current.entries, ...snapshot.entries]),
        );
      }
    } catch (error: unknown) {
      if (isCurrent(paneId, tabId, request)) {
        publish(paneId, tabId, {
          ...current,
          state: { type: 'error', message: errorMessage(error) },
          requestId: request.id,
        });
      }
    } finally {
      finish(paneId, tabId, request);
    }
  }

  function loadNextPage(paneId: PaneId, tabId?: TabId): Promise<void> {
    const workspace = options.getWorkspace();
    const resolvedTabId =
      tabId ?? (workspace === undefined ? undefined : activeTab(workspace, paneId)?.id);
    if (resolvedTabId === undefined) {
      return Promise.resolve();
    }
    const key = tabKey(paneId, resolvedTabId);
    const pending = pendingNextPage.get(key);
    if (pending !== undefined) {
      return pending;
    }
    const promise = loadNextPageImpl(paneId, resolvedTabId).finally(() => {
      pendingNextPage.delete(key);
    });
    pendingNextPage.set(key, promise);
    return promise;
  }

  async function loadAllPages(paneId: PaneId): Promise<void> {
    const workspace = options.getWorkspace();
    const tabId = workspace === undefined ? undefined : activeTab(workspace, paneId)?.id;
    if (tabId === undefined) {
      return;
    }
    for (;;) {
      // Stop (without switching targets) once the tab this was started for is no longer
      // active, e.g. the user switched tabs while pages were still loading in the background.
      const stillActive = options.getWorkspace();
      if (stillActive === undefined || activeTab(stillActive, paneId)?.id !== tabId) {
        return;
      }
      const current = paneViews.get(tabKey(paneId, tabId));
      if (current === undefined || !current.hasMore || current.state.type === 'error') {
        return;
      }
      await loadNextPage(paneId, tabId);
    }
  }

  return {
    load,
    navigate: (paneId, location, preferredCursorName) =>
      navigateHistory(paneId, 'push', location, preferredCursorName),
    parent: async (paneId) => {
      const workspace = options.getWorkspace();
      const tab = workspace === undefined ? undefined : activeTab(workspace, paneId);
      if (tab === undefined) {
        return;
      }
      const parent = parentLocation(tab.location);
      if (parent.uri !== tab.location.uri) {
        await navigateHistory(paneId, 'push', parent, lastPathSegment(tab.location));
      }
    },
    back: (paneId) => navigateHistory(paneId, 'back'),
    forward: (paneId) => navigateHistory(paneId, 'forward'),
    retry: load,
    loadNextPage,
    loadAllPages,
    abort: (paneId, tabId) => {
      const key = tabKey(paneId, tabId);
      activeRequests.get(key)?.controller.abort();
      clearBackgroundRetry(key);
    },
    moveTab: (sourcePaneId, targetPaneId, tabId) => {
      if (sourcePaneId === targetPaneId) return;
      const sourceKey = tabKey(sourcePaneId, tabId);
      const targetKey = tabKey(targetPaneId, tabId);
      activeRequests.get(sourceKey)?.controller.abort();
      activeRequests.delete(sourceKey);
      clearBackgroundRetry(sourceKey);
      pendingNextPage.delete(sourceKey);
      const backgroundFailures = backgroundFailureCounts.get(sourceKey);
      if (backgroundFailures !== undefined) {
        backgroundFailureCounts.delete(sourceKey);
        backgroundFailureCounts.set(targetKey, backgroundFailures);
      }
      const view = paneViews.get(sourceKey);
      if (view !== undefined) {
        paneViews.delete(sourceKey);
        paneViews.set(targetKey, view);
      }
    },
    dispose: () => {
      for (const request of activeRequests.values()) {
        request.controller.abort();
      }
      activeRequests.clear();
      for (const timer of backgroundRetryTimers.values()) {
        clearTimeout(timer);
      }
      backgroundRetryTimers.clear();
      backgroundFailureCounts.clear();
    },
  };
}
