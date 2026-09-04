import m from 'mithril';
import type { FileManagerClient } from '../../api/client/file-manager-client';
import { t } from '../../i18n';
import type { KeybindingRuntime } from '../../keybindings/dispatcher';
import type {
  ActionDescriptor,
  ActionInvocationContext,
  ClipboardState,
  Connection,
  EntryId,
  EntrySummary,
  Location,
  PaneId,
  PluginDescriptor,
  SavedSearch,
  SearchQuery,
  Settings,
  SortDescriptor,
  SystemLocation,
  TabId,
  TabProjection,
  Volume,
  WorkspaceProjection,
} from '../../models';
import {
  type AppState,
  applyAppPatches,
  deleteQuickFilterDraftPatch,
  setQuickFilterDraftPatch,
} from '../../state';
import { isCutLocation } from '../clipboard/clipboard';
import { loadConnections } from '../connections/connections-model';
import { SAMPLE_FILE_AGE_COLUMN } from '../directory-table/directory-table';
import type { FinderTagsLoader } from '../directory-table/finder-tags-loader';
import type { NativeIconLoader } from '../directory-table/native-icon-loader';
import type { ThumbnailLoader } from '../directory-table/thumbnail-loader';
import { DiskUsageView, type DiskUsageViewState } from '../disk-usage/disk-usage-view';
import { operationForDrop, resolveDropTarget, validateDropTarget } from '../drag-drop/drag-drop';
import { FileEditor } from '../editor/file-editor';
import type { FileEditorController, FileEditorState } from '../editor/file-editor-controller';
import type { EntryFormatSettings } from '../entry-formatting/entry-formatting';
import { isRestorableFavouriteLocation, reorderFavourites } from '../favourites/favourites';
import { archiveRootForEntry } from '../navigation/archive-location';
import {
  type NavigationController,
  type PaneDirectoryView,
  parentLocation,
} from '../navigation/navigation';
import type { OperationsController } from '../operations/operations-controller';
import { isParentEntry, withParentEntry } from '../panes/parent-entry';
import { FileViewer } from '../preview/file-viewer';
import type { FileViewerController, FileViewerState } from '../preview/file-viewer-controller';
import { hiddenSelectedEntryCount } from '../quick-filter/quick-filter';
import { saveSearch } from '../search/saved-searches';
import type { SelectionPlatform } from '../selection/keybindings';
import {
  emptySelection,
  reduceSelection,
  type SelectionAction,
  type SelectionState,
} from '../selection/selection';
import { type SortModel, sortEntriesResponsive } from '../sorting/sorting';
import { dispatchWorkspaceCommand } from './dispatch-workspace-command';
import type { WorkspaceController } from './workspace-controller';
import { pathFromUri, type WorkspacePaneContent } from './workspace-layout';

type InitialSearch = {
  readonly query: string;
  readonly regex: boolean;
  readonly caseSensitive: boolean;
  readonly wholeWord: boolean;
};

/** Shown before settings finish loading, mirroring the backend's own default (`core.gitStatus`
 * stays opt-in even then). */
const DEFAULT_VISIBLE_COLUMN_IDS: readonly string[] = [
  'core.name',
  'core.extension',
  'core.size',
  'core.modified',
];

export interface PaneContentContext {
  // Scalar state getters
  getWorkspace(): WorkspaceProjection | undefined;
  getCurrentSettings(): Settings | undefined;
  getSystemLocations(): readonly SystemLocation[];
  getSystemLocationsError(): string | undefined;
  getVolumes(): readonly Volume[];
  getVolumesError(): string | undefined;
  getConnections(): readonly Connection[];
  getUnavailableLocations(): ReadonlySet<string>;
  getNativeIconLoader(): NativeIconLoader | undefined;
  getThumbnailLoader(): ThumbnailLoader | undefined;
  getFinderTagsLoader(): FinderTagsLoader | undefined;
  getPlugins(): readonly PluginDescriptor[];
  getPlatform(): SelectionPlatform;
  getKeybindingRuntime(): KeybindingRuntime;
  getRegisteredActions(): readonly ActionDescriptor[];
  getDraggedLocations(): readonly Location[];
  getNativeDragOutSupported(): boolean;
  getNativeDropInProgress(): boolean;
  getAppState(): AppState | undefined;
  clipboard(): ClipboardState;

  // Map state (mutable reference — callers may .get()/.set()/.delete() directly)
  getDirectories(): Map<string, PaneDirectoryView>;
  getSelections(): Map<string, SelectionState>;
  getSortedEntries(): Map<
    string,
    {
      readonly input: readonly EntrySummary[];
      readonly key: string;
      readonly entries: readonly EntrySummary[];
    }
  >;
  getSortRequests(): Map<string, object>;
  /** Tokens guarding the async "moveCursorTo last" and typeahead-no-match background-load flows
   * — see onSelectionAction below. */
  getCursorLoadTokens(): Map<string, object>;
  getViewerByTab(): Map<
    string,
    {
      readonly paneId: PaneId;
      readonly tabId: TabId;
      readonly controller: FileViewerController;
      state: FileViewerState;
    }
  >;
  getEditorByPane(): Map<
    PaneId,
    { readonly controller: FileEditorController; state: FileEditorState }
  >;
  getDiskUsageByTab(): Map<string, { state: DiskUsageViewState }>;

  // Scalar state setters
  setConnections(conns: readonly Connection[]): void;
  setConnectionsManagerOpen(open: boolean): void;
  setAppState(state: AppState): void;
  setQuickFilterOpen(key: string, open: boolean): void;
  setDraggedLocations(locs: readonly Location[]): void;
  setNativeDragSourceInternal(v: boolean): void;
  setClipboardMessage(msg: string | undefined): void;
  setMultiRenameOpen(open: boolean): void;
  setMultiRenameEntries(entries: readonly EntrySummary[]): void;
  setMultiRenameLocation(location: Location | undefined): void;
  setMultiRenameExistingNames(names: ReadonlySet<string>): void;

  // Helper functions
  tabKey(paneId: PaneId, tabId: TabId): string;
  effectiveSort(sort: readonly SortDescriptor[]): readonly SortDescriptor[];
  frontendSort(sort: readonly SortDescriptor[]): SortModel;
  sortLabel(sort: readonly SortDescriptor[]): string;
  entriesSortedFor(
    key: string,
    entries: readonly EntrySummary[],
    sort: readonly SortDescriptor[],
    foldersFirst: boolean,
    groupByParentPath?: boolean,
  ): readonly EntrySummary[];
  entriesFilteredFor(
    key: string,
    entries: readonly EntrySummary[],
    query: string,
  ): readonly EntrySummary[];
  quickFilterQueryFor(key: string, tab: TabProjection | undefined): string;
  quickFilterOpenFor(key: string, tab: TabProjection | undefined): boolean;
  contentSearchInitialQuery(locationUri: string, entry: EntrySummary): InitialSearch | undefined;
  searchQueryForLocationUri(locationUri: string): SearchQuery | undefined;
  searchFavouriteNameForLocationUri(locationUri: string): string | undefined;
  workspaceErrorMessage(error: unknown, fallback: string): string;
  locationForPath(current: Location, path: string): Location;
  activeDirectory(): { paneId: PaneId; location: Location } | undefined;

  // Controller accessors
  getNavigation(): NavigationController;
  getWorkspaceController(): WorkspaceController;
  getOpsController(): OperationsController;
  openSavedSearch(saved: SavedSearch): void;

  // Action functions
  openViewer(
    paneId: PaneId,
    entry: EntrySummary,
    initialSearch?: InitialSearch,
    openMetadata?: boolean,
  ): void;
  closeViewer(paneId: PaneId): void;
  closeEditor(paneId: PaneId): void;
  updateLocationSettings(
    client: FileManagerClient,
    update: (settings: Settings) => Settings,
  ): Promise<void>;
  invokeActionById(actionId: string, params: unknown, ctx: ActionInvocationContext): void;
  openContextMenu(paneId: PaneId, entries: readonly EntrySummary[], x: number, y: number): void;
  refetchAffectedPanes(paneId?: PaneId): void;
  replaceWorkspace(next: WorkspaceProjection): void;
  openDiskUsageFolder(paneId: PaneId, location: Location): void;
  expandDiskUsageFolder(key: string, location: Location): void;
  retryDiskUsage(key: string): void;
  stopDiskUsage(key: string): void;
}

export function createPaneContentBuilder(
  context: PaneContentContext,
): (
  client: FileManagerClient,
  entryFormatSettings: EntryFormatSettings,
  paneId: PaneId,
) => WorkspacePaneContent {
  return function paneContent(
    client: FileManagerClient,
    entryFormatSettings: EntryFormatSettings,
    paneId: PaneId,
  ): WorkspacePaneContent {
    const workspace = context.getWorkspace();
    const pane = workspace?.panesById[paneId];
    const tab = pane?.tabsById[pane.activeTabId];
    const key = tab === undefined ? undefined : context.tabKey(paneId, tab.id);
    const directories = context.getDirectories();
    const selections = context.getSelections();
    const directory: PaneDirectoryView = (key === undefined ? undefined : directories.get(key)) ?? {
      state: { type: 'idle' } as const,
      entries: [],
      hasMore: false,
    };
    const selection = (key === undefined ? undefined : selections.get(key)) ?? emptySelection;
    const sorted =
      tab === undefined || key === undefined
        ? directory.entries
        : context.entriesSortedFor(
            key,
            directory.entries,
            context.effectiveSort(tab.view.sort),
            tab.view.foldersFirst,
            tab.location.uri.startsWith('search://'),
          );
    const quickFilterQuery = key === undefined ? '' : context.quickFilterQueryFor(key, tab);
    const filtered =
      key === undefined ? sorted : context.entriesFilteredFor(key, sorted, quickFilterQuery);
    const entries =
      tab === undefined ? filtered : withParentEntry(pathFromUri(tab.location.uri), filtered);
    const entryIds = entries.map((entry) => entry.id);
    // Shared by the "moveCursorTo last" and typeahead-no-match background flows below: both need
    // the fully-loaded, correctly sorted/filtered/parent-prefixed entry list once `loadAllPages`
    // resolves, not just the loaded-so-far prefix `entries` above was computed from.
    async function resolveFreshLoadedEntries(): Promise<readonly EntrySummary[]> {
      // `entriesSortedFor`'s cache is only refreshed by a redraw, and no redraw happens between
      // the background page fetches in `loadAllPages`. For directories at/over its 10k
      // responsive-sort threshold, reading the cache right now would otherwise return a stale
      // sort of a much smaller (pre-`loadAllPages`) prefix. Force a fresh, correctly-ordered sort
      // of the fully-loaded entries first, and seed the cache with it so this call (and the next
      // redraw) see the real order.
      if (key !== undefined && tab !== undefined) {
        const freshDirectory = context.getDirectories().get(key);
        if (freshDirectory !== undefined) {
          const sortDescriptors = context.effectiveSort(tab.view.sort);
          const groupByParentPath = tab.location.uri.startsWith('search://');
          const cacheKey = JSON.stringify([
            sortDescriptors,
            tab.view.foldersFirst,
            groupByParentPath,
          ]);
          // Invalidate any in-flight sort of an earlier (smaller) entries snapshot so it can't
          // overwrite the fresh result seeded below once it resolves.
          context.getSortRequests().set(key, {});
          const sorted = await sortEntriesResponsive(
            freshDirectory.entries,
            context.frontendSort(sortDescriptors),
            tab.view.foldersFirst,
            { groupByParentPath },
          );
          context.getSortedEntries().set(key, {
            input: freshDirectory.entries,
            key: cacheKey,
            entries: sorted,
          });
          context.getSortRequests().delete(key);
        }
      }
      const sortedFresh =
        tab === undefined || key === undefined
          ? (context.getDirectories().get(key ?? '')?.entries ?? [])
          : context.entriesSortedFor(
              key,
              context.getDirectories().get(key)?.entries ?? [],
              context.effectiveSort(tab.view.sort),
              tab.view.foldersFirst,
              tab.location.uri.startsWith('search://'),
            );
      const filteredFresh =
        key === undefined
          ? sortedFresh
          : context.entriesFilteredFor(key, sortedFresh, context.quickFilterQueryFor(key, tab));
      return tab === undefined
        ? filteredFresh
        : withParentEntry(pathFromUri(tab.location.uri), filteredFresh);
    }
    const cursorIndex =
      selection.cursorEntryId === undefined ? undefined : entryIds.indexOf(selection.cursorEntryId);
    const selectedEntryIds = new Set<EntryId>(selection.selectedEntryIds);
    // While filtering, the true directory total can't be projected past what's loaded and
    // matched so far; otherwise use the backend's real count (plus the synthetic ".." row)
    // so the scrollbar is sized correctly from the very first page, not just once fully loaded.
    const totalKnownEntries =
      quickFilterQuery.trim() === ''
        ? (directory.totalKnownEntries ?? directory.entries.length) +
          (entries.length - filtered.length)
        : entries.length;
    const currentSettings = context.getCurrentSettings();
    const systemLocationsError = context.getSystemLocationsError();
    const volumesError = context.getVolumesError();
    const nativeIconLoader = context.getNativeIconLoader();
    const thumbnailLoader = context.getThumbnailLoader();
    const finderTagsLoader = context.getFinderTagsLoader();
    const viewerTitles = new Map(
      (pane?.tabOrder ?? []).flatMap((tabId) => {
        const title = context.getViewerByTab().get(context.tabKey(paneId, tabId))?.state.entry.name;
        return title === undefined ? [] : [[tabId, title] as const];
      }),
    );
    for (const tabId of pane?.tabOrder ?? []) {
      if (context.getDiskUsageByTab().has(context.tabKey(paneId, tabId))) {
        viewerTitles.set(tabId, t('diskUsage', 'tabTitle'));
      }
    }
    const defaultFavouriteLabel =
      tab === undefined ? undefined : context.searchFavouriteNameForLocationUri(tab.location.uri);
    const currentSearchQuery =
      tab?.location.uri.startsWith('search://') === true
        ? context.searchQueryForLocationUri(tab.location.uri)
        : undefined;
    const currentLocationIsSavedSearch =
      currentSearchQuery !== undefined &&
      (currentSettings?.savedSearches.some(
        (saved) => JSON.stringify(saved.query) === JSON.stringify(currentSearchQuery),
      ) ??
        false);
    return {
      ...directory,
      viewerTitles,
      ...(tab === undefined ? {} : { location: tab.location }),
      ...(defaultFavouriteLabel === undefined ? {} : { defaultFavouriteLabel }),
      currentLocationIsSavedSearch,
      favouriteLocations:
        currentSettings?.favouriteLocations.filter(isRestorableFavouriteLocation) ?? [],
      recentLocations:
        workspace === undefined || currentSettings === undefined
          ? []
          : (currentSettings.recentLocationsByWorkspace[workspace.id] ?? []),
      savedSearches: currentSettings?.savedSearches ?? [],
      systemLocations: context.getSystemLocations(),
      ...(systemLocationsError === undefined ? {} : { systemLocationsError }),
      onRetrySystemLocations: () => context.getWorkspaceController().loadSystemLocations(),
      volumes: context.getVolumes(),
      ...(volumesError === undefined ? {} : { volumesError }),
      onRetryVolumes: () => context.getWorkspaceController().loadVolumes(),
      connections: context.getConnections(),
      onManageConnections: () => {
        context.setConnectionsManagerOpen(true);
        m.redraw();
      },
      onOpenSavedSearch: (saved) => context.openSavedSearch(saved),
      onRefreshConnections: async () => {
        context.setConnections(await loadConnections(client));
      },
      unavailableLocations: context.getUnavailableLocations(),
      entries,
      selectedEntryIds,
      cutEntryIds: new Set<EntryId>(
        directory.entries
          .filter((entry) => isCutLocation(context.clipboard(), entry.location))
          .map((entry) => entry.id),
      ),
      sortLabel: context.sortLabel(context.effectiveSort(tab?.view.sort ?? [])),
      sort: context.effectiveSort(tab?.view.sort ?? []),
      totalEntryCount: directory.entries.length,
      totalKnownEntries,
      hiddenSelectedCount: hiddenSelectedEntryCount(directory.entries, filtered, selectedEntryIds),
      filterOpen: key === undefined ? false : context.quickFilterOpenFor(key, tab),
      filterQuery: quickFilterQuery,
      formatSettings: entryFormatSettings,
      ...(nativeIconLoader === undefined ? {} : { nativeIconLoader }),
      ...(thumbnailLoader === undefined ? {} : { thumbnailLoader }),
      ...(finderTagsLoader === undefined ? {} : { finderTagsLoader }),
      pluginColumns: [
        ...(context
          .getPlugins()
          .some(
            (plugin) =>
              plugin.enabled && plugin.columns?.some((column) => column.id === 'sample.fileAge'),
          ) &&
        tab?.view.columns.some((column) => column.columnId === 'sample.fileAge' && column.visible)
          ? [SAMPLE_FILE_AGE_COLUMN]
          : []),
      ],
      // Column visibility is a single global setting, not persisted per tab (see
      // `columnWidths`/`onColumnWidthChange` below for the same treatment of widths).
      visibleColumnIds: new Set(currentSettings?.defaultColumns ?? DEFAULT_VISIBLE_COLUMN_IDS),
      // Only worth showing once the setting is explicitly on *and* this directory actually
      // reports git status - most users have no git projects, so it stays out of the way by
      // default.
      showGitStatusColumn:
        (currentSettings?.defaultColumns.includes('core.gitStatus') ?? false) &&
        directory.entries.some((entry) => entry.gitStatus !== undefined),
      platform: context.getPlatform(),
      keybindingRuntime: context.getKeybindingRuntime(),
      actions: context.getRegisteredActions(),
      keybindingOverrides: currentSettings?.keybindings ?? {},
      ...(cursorIndex === undefined || cursorIndex < 0 ? {} : { cursorIndex }),
      onNavigate: async (path) => {
        if (tab !== undefined) {
          await context
            .getNavigation()
            .navigate(paneId, context.locationForPath(tab.location, path));
        }
      },
      onNavigateLocation: async (location) => {
        await context.getNavigation().navigate(paneId, location);
      },
      onAddFavourite: (label, location) =>
        context.updateLocationSettings(client, (settings) => ({
          ...settings,
          ...(location.uri.startsWith('search://')
            ? (() => {
                const query = context.searchQueryForLocationUri(location.uri);
                if (query === undefined) return {};
                const existing = settings.savedSearches.find(
                  (saved) => JSON.stringify(saved.query) === JSON.stringify(query),
                );
                return {
                  favouriteLocations: settings.favouriteLocations.filter(
                    isRestorableFavouriteLocation,
                  ),
                  savedSearches: saveSearch(settings.savedSearches, {
                    id: existing?.id ?? crypto.randomUUID(),
                    name: label,
                    pinned: true,
                    query,
                  }),
                };
              })()
            : {
                favouriteLocations: [...settings.favouriteLocations, { label, location }],
              }),
        })),
      onDeleteFavourite: (location) =>
        context.updateLocationSettings(client, (settings) => ({
          ...settings,
          favouriteLocations: settings.favouriteLocations.filter(
            (favourite) =>
              favourite.location.providerId !== location.providerId ||
              favourite.location.uri !== location.uri,
          ),
          recentLocationsByWorkspace: Object.fromEntries(
            Object.entries(settings.recentLocationsByWorkspace).map(([workspaceId, locations]) => [
              workspaceId,
              locations.filter(
                (candidate) =>
                  candidate.providerId !== location.providerId || candidate.uri !== location.uri,
              ),
            ]),
          ),
        })),
      onReorderFavourites: (from, to) =>
        context.updateLocationSettings(client, (settings) => ({
          ...settings,
          favouriteLocations: reorderFavourites(
            settings.favouriteLocations.filter(isRestorableFavouriteLocation),
            from,
            to,
          ),
        })),
      onBack: () => context.getNavigation().back(paneId),
      onForward: () => context.getNavigation().forward(paneId),
      onParent: () =>
        tab?.location.uri.startsWith('search://')
          ? context.getNavigation().back(paneId)
          : context.getNavigation().parent(paneId),
      onOpenEntry: (entry) => {
        if (isParentEntry(entry.id)) {
          return tab?.location.uri.startsWith('search://')
            ? context.getNavigation().back(paneId)
            : context.getNavigation().parent(paneId);
        }
        if (tab?.location.uri.startsWith('search://')) {
          const initialSearch = context.contentSearchInitialQuery(tab.location.uri, entry);
          if (initialSearch !== undefined) {
            const otherPaneId = workspace?.paneOrder.find(
              (candidatePaneId) => candidatePaneId !== paneId,
            );
            if (otherPaneId) {
              return context.openViewer(otherPaneId, entry, initialSearch);
            }
          }
          return context
            .getNavigation()
            .navigate(paneId, parentLocation(entry.location), entry.name);
        }
        const systemLocations = context.getSystemLocations();
        const isSystemLocation = systemLocations.some(
          ({ location }) =>
            location.providerId === entry.location.providerId &&
            location.uri === entry.location.uri,
        );
        if (entry.kind === 'directory' || isSystemLocation) {
          return context.getNavigation().navigate(paneId, entry.location);
        }
        const archiveRoot = archiveRootForEntry(entry);
        if (archiveRoot !== undefined) return context.getNavigation().navigate(paneId, archiveRoot);
        return context.invokeActionById(
          'core.open',
          { uri: entry.location.uri },
          { paneId, selectedEntryIds: [entry.id], cursorEntryId: entry.id },
        );
      },
      onSelectionAction: (action: SelectionAction) => {
        if (key === undefined) return;
        if (action.type === 'moveCursorTo' && action.edge === 'last' && directory.hasMore) {
          // The loaded prefix doesn't include the real last entry yet: fetch every remaining
          // page (cheap, cache-backed slices on the backend) before landing the cursor, rather
          // than jumping to the last entry loaded so far.
          const loadToken = {};
          context.getCursorLoadTokens().set(key, loadToken);
          void context
            .getNavigation()
            .loadAllPages(paneId)
            .then(async () => {
              const loadedEntries = await resolveFreshLoadedEntries();
              const loadedEntryIds = loadedEntries.map((entry) => entry.id);
              // Drop this resolution if the user has since taken another selection action (e.g.
              // pressed Up while the pages were still loading) — applying it now would silently
              // snap the cursor back to the last entry and undo their more recent navigation.
              // This must stay scoped to `tab`/`key` as captured when the action was dispatched,
              // never re-derived from `pane.activeTabId` — the user may have switched to a
              // different tab while the pages were still loading in the background, and applying
              // the result to whichever tab is active *now* would write this tab's cursor/entry
              // into the wrong tab's selection state.
              if (context.getCursorLoadTokens().get(key) !== loadToken) return;
              const next = reduceSelection(
                context.getSelections().get(key) ?? selection,
                action,
                loadedEntryIds,
              );
              context.getSelections().set(key, next);
              m.redraw();
            });
          return;
        }
        if (action.type === 'typeaheadPending' && directory.hasMore) {
          // The directory isn't fully loaded yet, so the prefix may have matched only among the
          // entries loaded so far (or not at all) while a better/only match exists further in.
          // Background-load every remaining page (task: type-to-select only searching loaded
          // entries) and, once in, put the cursor on the true first match if one exists.
          const loadToken = {};
          context.getCursorLoadTokens().set(key, loadToken);
          void context
            .getNavigation()
            .loadAllPages(paneId)
            .then(async () => {
              const loadedEntries = await resolveFreshLoadedEntries();
              if (context.getCursorLoadTokens().get(key) !== loadToken) return;
              const match = loadedEntries.find((entry) =>
                entry.name.toLocaleLowerCase().includes(action.prefix),
              );
              if (match === undefined) return;
              const next = reduceSelection(
                context.getSelections().get(key) ?? selection,
                { type: 'setCursor', entryId: match.id },
                loadedEntries.map((entry) => entry.id),
              );
              context.getSelections().set(key, next);
              m.redraw();
            });
          return;
        }
        context.getCursorLoadTokens().delete(key);
        // Re-read the latest selection rather than the `selection` closed over when this attrs
        // object was built - other in-flight dispatches (e.g. a background `moveCursorTo`
        // resolution above) may have already updated it since this closure was created, and using
        // the stale value here would silently discard that update once this overwrites the map.
        const next = reduceSelection(
          context.getSelections().get(key) ?? selection,
          action,
          entryIds,
        );
        context.getSelections().set(key, next);
        // `m.redraw()` is throttled to the next animation frame, not synchronous. A plain
        // `m.redraw()` here left a window where a keypress arriving before that frame paints
        // (e.g. Space right after a click) still saw the *previous* render's `cursorIndex`/
        // `entries` props in the pane's `onkeydown` handler - toggling/advancing from the old
        // cursor position instead of the one this action just set. `sync()` forces the redraw to
        // land before this handler returns, so the very next keydown always sees fresh state.
        m.redraw.sync();
      },
      onRetry: () => context.getNavigation().retry(paneId),
      onLoadNextPage: () => context.getNavigation().loadNextPage(paneId),
      onSortChange: (sort) => {
        const liveWorkspace = context.getWorkspace();
        if (liveWorkspace === undefined || tab === undefined) return;
        void dispatchWorkspaceCommand(
          client,
          {
            type: 'updateView',
            workspaceId: liveWorkspace.id,
            paneId,
            tabId: tab.id,
            patch: { sort: [...sort] },
            expectedRevision: liveWorkspace.revision,
          },
          context.replaceWorkspace,
        ).catch(() => undefined);
      },
      viewMode: tab?.view.viewMode ?? 'table',
      iconSize: tab?.view.iconSize ?? 'medium',
      onViewModeChange: (viewMode, iconSize) => {
        const liveWorkspace = context.getWorkspace();
        if (liveWorkspace === undefined || tab === undefined) return;
        void dispatchWorkspaceCommand(
          client,
          {
            type: 'updateView',
            workspaceId: liveWorkspace.id,
            paneId,
            tabId: tab.id,
            patch: { viewMode, iconSize },
            expectedRevision: liveWorkspace.revision,
          },
          context.replaceWorkspace,
        ).catch(() => undefined);
      },
      // Column widths are a single global setting (not persisted per tab): resizing a column in
      // one tab is expected to apply to every tab and pane.
      columnWidths: Object.entries(currentSettings?.columnWidths ?? {}).map(
        ([columnId, width]) => ({ columnId, width }),
      ),
      onColumnWidthChange: (columnId, width) => {
        void context.updateLocationSettings(client, (settings) => ({
          ...settings,
          columnWidths: { ...settings.columnWidths, [columnId]: width },
        }));
      },
      onFilterQueryChange: (query) => {
        if (key === undefined) return;
        const appState = context.getAppState();
        if (appState === undefined) return;
        context.setAppState(applyAppPatches(appState, setQuickFilterDraftPatch(key, query)));
        m.redraw();
      },
      onFilterCommit: () => {
        if (key === undefined) return;
        const draft = context.getAppState()?.quickFilterDrafts.byTabKey[key];
        const liveWorkspace = context.getWorkspace();
        if (liveWorkspace === undefined || tab === undefined || draft === undefined) return;
        const committed = tab.view.quickFilter?.query ?? '';
        if (draft === committed) return;
        void dispatchWorkspaceCommand(
          client,
          {
            type: 'updateView',
            workspaceId: liveWorkspace.id,
            paneId,
            tabId: tab.id,
            patch: {
              quickFilter:
                draft.trim() === '' ? { type: 'clear' } : { type: 'set', filter: { query: draft } },
            },
            expectedRevision: liveWorkspace.revision,
          },
          context.replaceWorkspace,
        ).catch(() => undefined);
      },
      onFilterClose: () => {
        if (key !== undefined) {
          context.setQuickFilterOpen(key, false);
          const appState = context.getAppState();
          if (appState !== undefined) {
            context.setAppState(applyAppPatches(appState, deleteQuickFilterDraftPatch(key)));
          }
        }
        const liveWorkspace = context.getWorkspace();
        if (liveWorkspace !== undefined && tab !== undefined && tab.view.quickFilter != null) {
          void dispatchWorkspaceCommand(
            client,
            {
              type: 'updateView',
              workspaceId: liveWorkspace.id,
              paneId,
              tabId: tab.id,
              patch: { quickFilter: { type: 'clear' } },
              expectedRevision: liveWorkspace.revision,
            },
            context.replaceWorkspace,
          ).catch(() => undefined);
        }
        m.redraw();
      },
      onRename: (entry, name) => {
        const active = context.activeDirectory();
        if (active === undefined || active.paneId !== paneId) return;
        const destinationUri = `${active.location.uri.replace(/\/$/u, '')}/${encodeURIComponent(name)}`;
        void context
          .getOpsController()
          .rename(entry.location, { ...entry.location, uri: destinationUri });
      },
      onContextMenu: (entries, x, y) => context.openContextMenu(paneId, entries, x, y),
      onDragStart: (draggedEntries, event) => {
        context.setDraggedLocations(draggedEntries.map((entry) => entry.location));
        event.dataTransfer?.setData('application/x-fm-locations', 'internal');
        if (event.dataTransfer != null) event.dataTransfer.effectAllowed = 'copyMove';
      },
      ...(context.getNativeDragOutSupported()
        ? {
            onPointerDragStart: (draggedEntries) => {
              context.setDraggedLocations(draggedEntries.map((entry) => entry.location));
            },
            pointerDragEffect: (event) => operationForDrop(context.getPlatform(), event),
            onPointerDragOut: (draggedEntries) => {
              const locations = draggedEntries.map((entry) => entry.location);
              context.setDraggedLocations(locations);
              context.setNativeDragSourceInternal(true);
              void client.startNativeDrag(locations).catch((error: unknown) => {
                context.setClipboardMessage(
                  context.workspaceErrorMessage(error, 'Unable to start native drag'),
                );
                m.redraw();
              });
            },
          }
        : {}),
      onDragOver: (entry, event) => {
        const target = tab === undefined ? undefined : resolveDropTarget(tab.location, entry);
        const validation = validateDropTarget(
          context.getDraggedLocations(),
          target,
          directory.writable === true,
        );
        if (!validation.ok) return false;
        if (event.dataTransfer != null) {
          event.dataTransfer.dropEffect = operationForDrop(context.getPlatform(), event);
        }
        return true;
      },
      onDrop: (entry, event) => {
        if (tab === undefined) return;
        const target = resolveDropTarget(tab.location, entry);
        const validation = validateDropTarget(
          context.getDraggedLocations(),
          target,
          directory.writable === true,
        );
        if (!validation.ok) {
          context.setClipboardMessage(validation.message);
          return;
        }
        const sources = context.getDraggedLocations();
        context.setDraggedLocations([]);
        void (context.getNativeDropInProgress() ||
        operationForDrop(context.getPlatform(), event) === 'copy'
          ? context.getOpsController().copy(sources, target)
          : context.getOpsController().move(sources, target));
      },
      onTabDragOver: (targetTabId, event) => {
        const targetTab = pane?.tabsById[targetTabId];
        const targetDirectory = directories.get(context.tabKey(paneId, targetTabId));
        const validation = validateDropTarget(
          context.getDraggedLocations(),
          targetTab?.location,
          targetDirectory?.writable === true,
        );
        if (!validation.ok) return false;
        if (event.dataTransfer != null)
          event.dataTransfer.dropEffect = operationForDrop(context.getPlatform(), event);
        return true;
      },
      onTabDrop: (targetTabId, event) => {
        const targetTab = pane?.tabsById[targetTabId];
        const targetDirectory = directories.get(context.tabKey(paneId, targetTabId));
        const validation = validateDropTarget(
          context.getDraggedLocations(),
          targetTab?.location,
          targetDirectory?.writable === true,
        );
        if (!validation.ok || targetTab === undefined) {
          if (!validation.ok) context.setClipboardMessage(validation.message);
          return;
        }
        const sources = context.getDraggedLocations();
        context.setDraggedLocations([]);
        void (context.getNativeDropInProgress() ||
        operationForDrop(context.getPlatform(), event) === 'copy'
          ? context.getOpsController().copy(sources, targetTab.location)
          : context.getOpsController().move(sources, targetTab.location));
      },
      onMultiRename: (selected) => {
        if (tab === undefined) return;
        context.setMultiRenameOpen(true);
        context.setMultiRenameEntries(selected);
        context.setMultiRenameLocation(tab.location);
        const selectedIds = new Set(selected.map((entry) => entry.id));
        context.setMultiRenameExistingNames(
          new Set(
            directory.entries
              .filter((entry) => !selectedIds.has(entry.id))
              .map((entry) => entry.name),
          ),
        );
      },
      ...(context.getEditorByPane().has(paneId)
        ? {
            viewerContent: (() => {
              const editor = context.getEditorByPane().get(paneId);
              return editor === undefined
                ? undefined
                : m(FileEditor, {
                    state: editor.state,
                    controller: editor.controller,
                    paneId,
                    onClose: () => context.closeEditor(paneId),
                  });
            })(),
          }
        : key !== undefined && context.getDiskUsageByTab().has(key)
          ? {
              viewerContent: (() => {
                const diskUsage = context.getDiskUsageByTab().get(key);
                return diskUsage === undefined
                  ? undefined
                  : m(DiskUsageView, {
                      state: diskUsage.state,
                      onOpenFolder: (location) => context.openDiskUsageFolder(paneId, location),
                      onExpandFolder: (location) => context.expandDiskUsageFolder(key, location),
                      onRetry: () => context.retryDiskUsage(key),
                      onStop: () => context.stopDiskUsage(key),
                    });
              })(),
            }
          : key !== undefined && context.getViewerByTab().has(key)
            ? {
                viewerContent: (() => {
                  const viewer = context.getViewerByTab().get(key);
                  if (viewer === undefined) return undefined;
                  const videoPosterDataUri = context
                    .getThumbnailLoader()
                    ?.thumbnailDataUri(viewer.state.entry, 'large');
                  const quickLookAvailable =
                    viewer.state.entry.kind === 'file' &&
                    viewer.state.entry.location.providerId === 'local' &&
                    viewer.state.entry.location.uri.startsWith('file://') &&
                    context
                      .getRegisteredActions()
                      .some(
                        (action) =>
                          action.id === 'core.quickLook' &&
                          action.contextRequirements.featureAvailable !== false,
                      );
                  return m(FileViewer, {
                    state: viewer.state,
                    onLoadMore: () => void viewer.controller.loadMore(),
                    onLoadPrevious: () => void viewer.controller.loadPrevious(),
                    onLoadTextPage: (pageIndex) => void viewer.controller.loadTextPage(pageIndex),
                    onLoadStructuredRows: (startRow) =>
                      void viewer.controller.loadStructuredRows(startRow),
                    onStructuredOptionsChange: (delimiter, headerMode) =>
                      void viewer.controller.setStructuredOptions(delimiter, headerMode),
                    onSelectStructuredSheet: (sheetName) =>
                      void viewer.controller.selectStructuredSheet(sheetName),
                    onToggleStructuredRowNumbers: () =>
                      viewer.controller.toggleStructuredRowNumbers(),
                    onLoadJsonWindow: (offset) => void viewer.controller.loadJsonWindow(offset),
                    onSearchStructuredRows: (query, cursor) =>
                      void viewer.controller.searchStructuredRows(query, cursor),
                    onSortStructuredRows: (column) =>
                      void viewer.controller.sortStructuredRows(column),
                    onSearchQueryChange: (query) => viewer.controller.setSearchOptions({ query }),
                    onSearchOptionChange: (patch) => viewer.controller.setSearchOptions(patch),
                    onRunSearch: () => void viewer.controller.runSearch(),
                    onNextMatch: () => void viewer.controller.goToNextMatch(),
                    onPreviousMatch: () => void viewer.controller.goToPreviousMatch(),
                    onZoomIn: () => viewer.controller.zoomIn(),
                    onZoomOut: () => viewer.controller.zoomOut(),
                    onZoomChange: (zoom) => viewer.controller.setZoom(zoom),
                    onResetZoom: () => viewer.controller.resetZoom(),
                    onCopy: () => viewer.controller.copyContent(),
                    onToggleMetadata: () => viewer.controller.toggleMetadataPanel(),
                    onNextPage: () => viewer.controller.nextPage(),
                    onPreviousPage: () => viewer.controller.previousPage(),
                    onPdfSearchQueryChange: (query) => viewer.controller.setPdfSearchQuery(query),
                    onNextPdfMatch: () => viewer.controller.goToNextPdfMatch(),
                    onPreviousPdfMatch: () => viewer.controller.goToPreviousPdfMatch(),
                    onEpubSearchQueryChange: (query) => viewer.controller.setEpubSearchQuery(query),
                    onNextEpubMatch: () => viewer.controller.goToNextEpubMatch(),
                    onPreviousEpubMatch: () => viewer.controller.goToPreviousEpubMatch(),
                    onSelectEpubSection: (sectionIndex, fragment) =>
                      viewer.controller.goToEpubSection(sectionIndex, fragment),
                    onFollowEpubLink: (href) => viewer.controller.followEpubLink(href),
                    onSelectPdfPage: (pageNumber) => viewer.controller.goToPdfPage(pageNumber),
                    onNavigateTextOffset: (offset, length) =>
                      viewer.controller.goToTextOffset(offset, length),
                    onOpenExternalLink: (url) => void client.openExternalUrl(url),
                    ...(videoPosterDataUri === undefined ? {} : { videoPosterDataUri }),
                    quickLookAvailable,
                    onQuickLook: () =>
                      context.invokeActionById(
                        'core.quickLook',
                        { uri: viewer.state.entry.location.uri },
                        {
                          paneId,
                          selectedEntryIds: [viewer.state.entry.id],
                          cursorEntryId: viewer.state.entry.id,
                        },
                      ),
                    onOpenExternally: () =>
                      context.invokeActionById(
                        'core.open',
                        { uri: viewer.state.entry.location.uri },
                        {
                          paneId,
                          selectedEntryIds: [viewer.state.entry.id],
                          cursorEntryId: viewer.state.entry.id,
                        },
                      ),
                    onClose: () => context.closeViewer(paneId),
                  });
                })(),
              }
            : {}),
    };
  };
}
