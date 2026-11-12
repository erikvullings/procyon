/**
 * The application shell's mutable state (task 0290).
 *
 * `AppShell` used to hold every piece of shell state in ~170 closure-level `let`/`const`
 * bindings. A closure binding cannot be shared: the only way to reach it from another module was
 * to hand-write a `getX`/`setX` pair per field, which is exactly why the shell's wiring grew back
 * into one 8 000-line file after every extraction. This module turns that state into one
 * explicitly-typed object created per shell instance, so a wiring module can take `state` (and
 * the handful of collaborators it actually needs) instead of a wall of accessors.
 *
 * Field semantics are unchanged: a field annotated `X | undefined` is the shell's "not set yet",
 * mutations stay local to the feature that owns them, and nothing here touches the network.
 */
import type { Theme } from 'mithril-materialized';

import type { FileManagerClient } from '../api/client/file-manager-client';
import type { ActionCommandController } from '../features/actions/action-command-controller';
import type { AssistantTabEntry } from '../features/assistant/assistant-tab-state';
import type { ChangeAttributesRequest } from '../features/attributes/change-attributes-dialog';
import type { ChatTabEntry } from '../features/chat/chat-tab-state';
import type { ChecksumController } from '../features/checksums/checksum-controller';
import type { ChecksumState, DuplicateState } from '../features/checksums/checksum-state';
import { initialChecksumState, initialDuplicateState } from '../features/checksums/checksum-state';
import { createOsFileClipboard } from '../features/clipboard/os-file-clipboard';
import type { ComparisonController } from '../features/comparison/comparison-controller';
import {
  type ComparisonState,
  initialComparisonState,
} from '../features/comparison/comparison-state';
import { createDialogUIController } from '../features/dialogs/dialog-ui-controller';
import type { FinderTagsLoader } from '../features/directory-table/finder-tags-loader';
import type { NativeIconLoader } from '../features/directory-table/native-icon-loader';
import type { ThumbnailLoader } from '../features/directory-table/thumbnail-loader';
import type { TreeChildrenState } from '../features/directory-tree/directory-tree-state';
import { createTreeChildrenState } from '../features/directory-tree/directory-tree-state';
import type { DiskUsageViewState } from '../features/disk-usage/disk-usage-view';
import type {
  FileEditorController,
  FileEditorState,
} from '../features/editor/file-editor-controller';
import type { EntryFormatSettings } from '../features/entry-formatting/entry-formatting';
import { DEFAULT_ENTRY_FORMAT_SETTINGS } from '../features/entry-formatting/entry-formatting';
import type { FileDiffRequest } from '../features/file-diff/file-diff-dialog';
import type { ConvertGeoRequest } from '../features/geo/convert-geo-dialog';
import type { MinifyGeoRequest } from '../features/geo/minify-geo-dialog';
import type { ImageMapController } from '../features/image-map/image-map-controller';
import { type ImageMapState, INITIAL_IMAGE_MAP_STATE } from '../features/image-map/image-map-state';
import type { EditImageMetadataRequest } from '../features/image-metadata/edit-image-metadata-dialog';
import type { LinkDoctorController } from '../features/link-doctor/link-doctor-controller';
import {
  INITIAL_LINK_DOCTOR_STATE,
  type LinkDoctorState,
} from '../features/link-doctor/link-doctor-state';
import type { MusicDuplicatesController } from '../features/music-duplicates/music-duplicates-controller';
import {
  INITIAL_MUSIC_DUPLICATES_STATE,
  type MusicDuplicatesState,
} from '../features/music-duplicates/music-duplicates-state';
import type { NavigationController, PaneDirectoryView } from '../features/navigation/navigation';
import type { SyncedBrowsingController } from '../features/navigation/synced-browsing';
import { createOperationsState } from '../features/operations/operation-state';
import type {
  OperationConfirmationDecision,
  OperationConfirmationRequest,
  OperationsController,
} from '../features/operations/operations-controller';
import type { PaneRenameRequest } from '../features/panes/pane';
import type { TabController } from '../features/panes/tab-controller';
import type { PlayerController } from '../features/player/player-controller';
import { createInitialPlayerState } from '../features/player/player-state';
import type { SavePlaylistRequest } from '../features/playlist/save-playlist-dialog';
import type { PluginPaneState } from '../features/plugins/plugin-panel-host';
import type {
  FileViewerController,
  FileViewerState,
} from '../features/preview/file-viewer-controller';
import type { AdvancedFilterCriteria } from '../features/quick-filter/advanced-filter';
import { AdvancedFilterStore } from '../features/quick-filter/advanced-filter-store';
import type { FindFilesController } from '../features/search/find-files-controller';
import type { FindFilesSearchParams } from '../features/search/find-files-dialog';
import type { SearchPresentation } from '../features/search/search-presentation';
import type { SelectionPlatform } from '../features/selection/keybindings';
import type { SelectionAction, SelectionState } from '../features/selection/selection';
import type { SettingsController } from '../features/settings/settings-controller';
import type { SettingsSection } from '../features/settings/settings-editor';
import type { SaveSnapshotRequest } from '../features/snapshots/save-snapshot-dialog';
import type { SnapshotController } from '../features/snapshots/snapshot-controller';
import {
  INITIAL_SNAPSHOT_PANEL_STATE,
  type SnapshotPanelState,
} from '../features/snapshots/snapshot-state';
import type { CombineFilesRequest } from '../features/split/combine-files-dialog';
import type { SplitFileRequest } from '../features/split/split-file-dialog';
import type { SyncProfileController } from '../features/sync-profiles/sync-profile-controller';
import type { WalkthroughId } from '../features/walkthroughs/walkthroughs';
import type { WorkspaceController } from '../features/workspace/workspace-controller';
import type { WorkspacePaneContent } from '../features/workspace/workspace-layout';
import type { FunctionKeyModifiers, KeybindingRuntime } from '../keybindings/dispatcher';
import type {
  ActionDescriptor,
  AppUpdateInfo,
  BackendEvent,
  Connection,
  EntrySummary,
  InterruptedTransfer,
  Location,
  NativeMenuSpec,
  OperationConflict,
  OperationId,
  PaneId,
  PluginDescriptor,
  RagScope,
  RuntimeCapabilities,
  ScanDiskUsageResult,
  SearchExecutionMode,
  SearchQuery,
  Settings,
  SystemLocation,
  TabId,
  Volume,
  WorkspaceId,
  WorkspaceProjection,
  WorkspaceSummary,
} from '../models';
import type { AppState } from '../state';
import type { RuntimeKind } from '../utilities/runtime';
import { dedupeWindowTitleSink } from '../utilities/window-title-sink';
import { DEFAULT_THEME, loadDismissedOperationIds } from './shell-helpers';

/** A `Scan Disk Usage` tab session: the scan it owns, its progress and its expansion context. */
export interface DiskUsageTabEntry {
  key: string;
  paneId: PaneId;
  readonly tabId: TabId;
  readonly location: Location;
  abort: AbortController;
  scanId: string;
  progressComplete: boolean;
  expansionLocation: Location | undefined;
  expansionBaseResult: ScanDiskUsageResult | undefined;
  state: DiskUsageViewState;
}

/** A Knowledge Search session tab (task 0205). */
export interface KnowledgeSearchTabEntry {
  key: string;
  paneId: PaneId;
  readonly tabId: TabId;
  readonly workspaceId: WorkspaceId;
  readonly currentFolder: Location | undefined;
  readonly semanticSourceIds: readonly string[];
  readonly initialSubject: string | undefined;
}

/** A Themes tab over the library (and, when shown, one folder). */
export interface ThemesTabEntry {
  key: string;
  paneId: PaneId;
  readonly tabId: TabId;
  readonly scopes: readonly RagScope[];
}

/** The pane content factory the workspace layout renders each pane with. */
export type ShellPaneContentFactory = (
  client: FileManagerClient,
  entryFormatSettings: EntryFormatSettings,
  paneId: PaneId,
) => import('../features/workspace/workspace-layout').WorkspacePaneContent;

/** Every piece of mutable state one `AppShell` instance owns, created once per instance. */
export function createShellState(client: FileManagerClient) {
  return {
    /** The transport-neutral client this shell instance talks to. */
    client,
    /* STATE */
    theme: DEFAULT_THEME as Theme,
    currentSettings: undefined as unknown as Settings | undefined,
    settingsUpdateQueue: Promise.resolve(),
    settingsDisclosureElement: undefined as unknown as HTMLDetailsElement | undefined,
    settingsDialogOpen: false,
    /** Whether the compact toolbar's overflow menu is open. */
    toolbarOverflowOpen: false,
    semanticInstallation: undefined as unknown as
      | { state: 'installing' | 'succeeded' | 'failed'; message: string }
      | undefined,
    activityOpen: false,
    semanticJobs: new Map<
      string,
      Extract<BackendEvent['payload'], { type: 'semantic.ingestionProgress' }>
    >(),
    settingsInitialSection: 'appearance' as SettingsSection,
    availableAppUpdate: undefined as unknown as AppUpdateInfo | undefined,
    aboutDialogOpen: false,
    workspaceDisclosureElement: undefined as unknown as HTMLDetailsElement | undefined,
    registeredActions: [] as readonly ActionDescriptor[],
    systemLocations: [] as readonly SystemLocation[],
    systemLocationsError: undefined as unknown as string | undefined,
    volumes: [] as readonly Volume[],
    volumesError: undefined as unknown as string | undefined,
    /** Native home-directory path, for expanding a leading `~` typed into an address bar. */
    homeDirectory: undefined as unknown as string | undefined,
    unavailableLocations: new Set<string>(),
    plugins: [] as readonly PluginDescriptor[],
    connections: [] as readonly Connection[],
    connectionsManagerOpen: false,
    shortcutsHelpOpen: false,
    walkthroughsOpen: false,
    walkthroughInvitationVisible: false,
    selectedWalkthrough: 'basics' as WalkthroughId,
    semanticAssistantAvailable: false,
    knowledgeSearchAvailable: false,
    /** Themes cluster embeddings, so full-text alone cannot serve them. */
    themesAvailable: false,
    themesWorkspaceIndexed: false,
    knowledgeSearchChecked: false,
    knowledgeAnswerRevision: 0,
    semanticEnrolmentRequest: undefined as unknown as
      | { readonly workspaceId: WorkspaceId; readonly location: Location }
      | undefined,
    semanticEnrolmentReturnToAsk: false,
    functionKeyModifiers: {} as FunctionKeyModifiers,
    /** Last non-empty Quick Filter query per tab key, for the Ctrl+Shift+S "reactivate" shortcut. */
    lastQuickFilterQueryByTabKey: new Map<string, string>(),
    /** Serialized form of the last spec pushed to the native menu bar - `syncNativeMenu` diffs
     * against this so an unchanged spec never re-triggers `set_native_menu` (the menu bar is
     * desktop-only chrome, not app state, so a cheap `JSON.stringify` comparison is an acceptable
     * fallback given there is no existing deep-equal utility in this codebase to reuse). */
    lastSentNativeMenuSpecJson: undefined as unknown as string | undefined,
    windowsNativeMenuSpec: { menus: [] } as NativeMenuSpec,
    /** Set once `subscribe_native_menu_actions` has actually resolved. `syncNativeMenu` must not
     * push a spec before this: `set_native_menu` binds whatever channel is *currently* subscribed
     * as the click callback, and installs are memoized by spec content - if the very first push
     * raced ahead of the subscribe call, the backend would bind a no-op callback (nothing
     * subscribed yet) and then never rebuild the menu again to pick up the real one, since nothing
     * about the spec's content changes once subscribed. Every menu click would silently no-op. */
    nativeMenuChannelReady: false,
    /** Pushes the full native menu bar spec to the backend whenever it might have changed. Called
     * from the view rather than threaded through every individual mutation site (registered
     * actions/settings/favourites/workspace tabs are each reassigned from several different
     * closures across this file) - safe and cheap because of the diff above, and it can never miss
     * a state change that affects the menu. */
    /** Re-derives the native window title (the Dock menu entry) from the workspace each render;
     * the sink only fires when the formatted title changes. */
    windowTitleSink: dedupeWindowTitleSink(() => undefined),
    installedIconThemeId: undefined as unknown as string | undefined,
    keybindingRuntime: 'browser' as KeybindingRuntime,
    runtimeKind: 'http' as RuntimeKind,
    loadedEntryFormatSettings: DEFAULT_ENTRY_FORMAT_SETTINGS as EntryFormatSettings,
    currentEntryFormatSettings: DEFAULT_ENTRY_FORMAT_SETTINGS as EntryFormatSettings,
    workspace: undefined as unknown as WorkspaceProjection | undefined,
    workspaceError: undefined as unknown as string | undefined,
    workspaceSummaries: [] as readonly WorkspaceSummary[],
    workspaceActionError: undefined as unknown as string | undefined,
    flushPendingLayoutUpdate: undefined as unknown as (() => void) | undefined,
    dialogs: createDialogUIController(),
    findFilesOpen: false,
    findFilesRoot: undefined as unknown as Location | undefined,
    findFilesSearchId: undefined as unknown as string | undefined,
    findFilesTargetPaneBySearchId: new Map<string, PaneId>(),
    findFilesError: undefined as unknown as string | undefined,
    findFilesGeneration: 0,
    findFilesRootsByLocationUri: new Map<string, Location>(),
    /** Friendly kind and term for each search location's tab and breadcrumb labels. */
    findFilesPresentationsByLocationUri: new Map<string, SearchPresentation>(),
    /** Full params are retained separately so F3 can initialize content highlighting. */
    findFilesParamsByLocationUri: new Map<string, FindFilesSearchParams>(),
    findFilesQueriesByLocationUri: new Map<string, SearchQuery>(),
    findFilesExecutionModesBySearchId: new Map<string, SearchExecutionMode>(),
    pendingFindFilesStarts: 0,
    deferredSearchResultBatches: new Map<string, BackendEvent[]>(),
    /** Live directory-comparison overlay state (task 0075). Marks differing entries selected in
     * both panes once a comparison completes, Total-Commander-style, rather than surfacing a
     * separate review dialog. */
    comparisonState: initialComparisonState() as ComparisonState,
    /** Live checksum-job and duplicate-scan state (spec §18, task 0077). */
    checksumState: initialChecksumState() as ChecksumState,
    duplicateState: initialDuplicateState() as DuplicateState,
    /** Registered by `WorkspaceLayoutView` (task 0089): moves DOM focus into a pane so keyboard
     * cursor navigation works immediately, e.g. right after a filename search closes its dialog. */
    focusPane: undefined as unknown as ((paneId: PaneId) => void) | undefined,
    focusTerminal: undefined as unknown as (() => boolean) | undefined,
    /** Registered by `DirectoryTree` (task 0139): moves DOM focus into the tree sidebar. */
    focusDirectoryTree: undefined as unknown as (() => boolean) | undefined,
    commandPaletteOpen: false,
    /** Composite `paneId:tabId` keys of tabs with an open terminal drawer; a terminal stays bound
     * to the tab that opened it, not the folder it happened to be showing at the time. */
    openTerminalTabKeys: new Set<string>(),
    basketTabIds: new Set<TabId>(),
    basketRefreshTimer: undefined as unknown as ReturnType<typeof setTimeout> | undefined,
    disposeTerminalTab: undefined as unknown as ((tabKey: string) => void) | undefined,
    /** Directory-tree sidebar (task 0139): open/closed, lazily-fetched expansion/children cache,
     * the provider root it is currently rooted at, and the active-pane location it was last
     * synced to (so `syncDirectoryTreeToActiveLocation` only does work when that location
     * actually changes, regardless of which action changed it - navigate, breadcrumb, favourite,
     * history, tab switch, or pane switch). */
    treeSidebarOpen: false,
    treeState: createTreeChildrenState() as TreeChildrenState,
    treeLoadGeneration: 0,
    treeNeedsRefresh: false,
    treeRootLocation: undefined as unknown as Location | undefined,
    treeSyncedLocationUri: undefined as unknown as string | undefined,
    openTerminalSupported: false,
    platformContextMenuSupported: false,
    nativeIconLoader: undefined as unknown as NativeIconLoader | undefined,
    nativeIconLoaderSource: undefined as unknown as NativeIconLoader | undefined,
    thumbnailLoader: undefined as unknown as ThumbnailLoader | undefined,
    finderTagsLoader: undefined as unknown as FinderTagsLoader | undefined,
    /** Runtime capabilities discovered once at bootstrap (task 0145), used to capability-gate
     * affordances like the Properties dialog's Tags/Comment sections. */
    runtimeCapabilities: undefined as RuntimeCapabilities | undefined,
    contextMenu: undefined as unknown as
      | {
          readonly paneId: PaneId;
          readonly entries: readonly EntrySummary[];
          readonly x: number;
          readonly y: number;
        }
      | undefined,
    commandPaletteRecency: new Map<string, number>(),
    /**
     * Every per-tab runtime cache below is keyed by a composite `${paneId}:${tabId}`
     * string (see {@link tabKey}) rather than by `PaneId` alone, so switching tabs
     * never bleeds one tab's directory/selection/sort/filter state into another's
     * (spec §37).
     */
    directories: new Map<string, PaneDirectoryView>(),
    selections: new Map<string, SelectionState>(),
    /** Each pane's selection dispatcher, so the command palette can apply selection actions. */
    selectionDispatchers: new Map<PaneId, (action: SelectionAction) => void>(),
    pendingCursorRestorations: new Map<
      string,
      {
        providerId: string;
        uri: string;
        kind: EntrySummary['kind'];
        fallbackCursorId: SelectionState['cursorEntryId'];
        wasSelected: boolean;
        wasAnchor: boolean;
        expiresAt: number;
      }
    >(),
    /** The most recently started recursive folder-size walk (task 0071, Ctrl+.) - starting a new
     * one aborts whatever the previous one was still doing, since only one result is ever shown. */
    folderSizeCalculation: undefined as unknown as AbortController | undefined,
    /** Lister sessions are owned by tabs, so switching tabs never closes or obscures them. */
    viewerByTab: new Map<
      string,
      {
        readonly paneId: PaneId;
        readonly tabId: TabId;
        readonly controller: FileViewerController;
        readonly knowledgeSourceId?: string;
        state: FileViewerState;
      }
    >(),
    diskUsageByTab: new Map<string, DiskUsageTabEntry>(),
    knowledgeSearchByTab: new Map<string, KnowledgeSearchTabEntry>(),
    themesByTab: new Map<string, ThemesTabEntry>(),
    themesOpening: false,
    assistantByTab: new Map<string, AssistantTabEntry>(),
    assistantOpening: false,
    chatByTab: new Map<string, ChatTabEntry>(),
    chatOpening: false,
    knowledgeSearchOpening: false,
    editorByPane: new Map<
      PaneId,
      { readonly controller: FileEditorController; state: FileEditorState }
    >(),
    pluginByPane: new Map<PaneId, PluginPaneState>(),
    openingPluginPanes: new Set<PaneId>(),
    nextPluginPanelId: 0,
    sortedEntries: new Map<
      string,
      {
        readonly input: readonly EntrySummary[];
        readonly key: string;
        readonly entries: readonly EntrySummary[];
      }
    >(),
    sortRequests: new Map<string, object>(),
    /* Tokens guarding the async "moveCursorTo last" flow (pane-content-builder's onSelectionAction):*/
    /* loading every page before landing the cursor takes real time, and if the user issues another*/
    /* selection action (e.g. presses Up) before it resolves, the stale resolution must not clobber*/
    /* whatever the newer action already set. Cleared/replaced by pane-content-builder itself.*/
    cursorLoadTokens: new Map<string, object>(),
    /** Whether the inline quick-filter box is shown for a pane, independent of a persisted query. */
    quickFilterOpen: new Map<string, boolean>(),
    /** Per-tab recursive quick-filter scope (task 0310): whether "this folder + subfolders" is on,
     * the in-flight walk's AbortController, its latest collected results, and the base entries the
     * walk started from. Keyed by tab key (`paneId:tabId`). */
    quickFilterRecursive: new Map<
      string,
      {
        recursive: boolean;
        controller: AbortController | undefined;
        results: readonly EntrySummary[] | undefined;
        base: readonly EntrySummary[];
      }
    >(),
    /** Structured (advanced) filter criteria and panel state per tab (task 0169). */
    advancedFilters: new AdvancedFilterStore(),
    filteredEntries: new Map<
      string,
      {
        readonly input: readonly EntrySummary[];
        readonly query: string;
        readonly criteria: AdvancedFilterCriteria;
        readonly entries: readonly EntrySummary[];
      }
    >(),
    /** Pending confirmation for closing a pane's only remaining tab (spec §37). */
    closeTabConfirmation: undefined as unknown as
      | { readonly paneId: PaneId; readonly tabId: TabId }
      | undefined,
    platform: 'unknown' as SelectionPlatform,
    nativeDragOutSupported: false,
    nativeDropInProgress: false,
    nativeDragSourceInternal: false,
    draggedLocations: [] as readonly Location[],
    workspaceRequest: undefined as unknown as AbortController | undefined,
    unsubscribeEvents: undefined as unknown as (() => void) | undefined,
    unsubscribeNativeFileDrops: undefined as unknown as (() => void) | undefined,
    unsubscribeConnection: undefined as unknown as (() => void) | undefined,
    unsubscribeResynchronise: undefined as unknown as (() => void) | undefined,
    appState: undefined as unknown as AppState | undefined,
    playerController: undefined as unknown as PlayerController | undefined,
    savePlaylistRequest: undefined as unknown as SavePlaylistRequest | undefined,
    editImageMetadataRequest: undefined as unknown as EditImageMetadataRequest | undefined,
    changeAttributesRequest: undefined as unknown as ChangeAttributesRequest | undefined,
    convertGeoRequest: undefined as unknown as ConvertGeoRequest | undefined,
    splitFileRequest: undefined as unknown as SplitFileRequest | undefined,
    fileDiffRequest: undefined as unknown as FileDiffRequest | undefined,
    syncProfilesDialogOpen: false,
    combineFilesRequest: undefined as unknown as CombineFilesRequest | undefined,
    minifyGeoRequest: undefined as unknown as MinifyGeoRequest | undefined,
    imageMapState: INITIAL_IMAGE_MAP_STATE as ImageMapState,
    imageMapController: undefined as unknown as ImageMapController | undefined,
    /** The pane whose folder feeds the open map panel; used to reveal and highlight entries. */
    imageMapPaneId: undefined as unknown as PaneId | undefined,
    /** The tab that hosts the map: a transient tab in the pane opposite the folder being mapped. */
    imageMapTab: undefined as unknown as { paneId: PaneId; tabId: TabId } | undefined,
    musicDuplicatesController: undefined as unknown as MusicDuplicatesController | undefined,
    linkDoctorController: undefined as unknown as LinkDoctorController | undefined,
    snapshotController: undefined as unknown as SnapshotController | undefined,
    snapshotState: INITIAL_SNAPSHOT_PANEL_STATE as SnapshotPanelState,
    saveSnapshotRequest: undefined as unknown as SaveSnapshotRequest | undefined,
    linkDoctorState: INITIAL_LINK_DOCTOR_STATE as LinkDoctorState,
    musicDuplicatesState: INITIAL_MUSIC_DUPLICATES_STATE as MusicDuplicatesState,
    emptyPlayerState: createInitialPlayerState(),
    operations: createOperationsState(),
    /** Transfers the previous session left interrupted, awaiting a decision (task 0300). */
    interruptedTransfers: [] as readonly InterruptedTransfer[],
    interruptedTransferBusyId: undefined as unknown as OperationId | undefined,
    interruptedTransferError: undefined as unknown as string | undefined,
    /** Failed operations whose low-space dialog was already answered (task 0272). */
    answeredSpaceShortfalls: new Set<OperationId>(),
    pendingConflict: undefined as unknown as OperationConflict | undefined,
    clipboardMessage: undefined as unknown as string | undefined,
    pendingOperationEvents: [] as BackendEvent[],
    operationFrame: undefined as unknown as number | undefined,
    autoDismissTimers: new Map<OperationId, ReturnType<typeof setTimeout>>(),
    dismissedOperationIds: loadDismissedOperationIds(),
    removed: false,
    systemThemeQuery: (typeof window.matchMedia === 'function'
      ? window.matchMedia('(prefers-color-scheme: dark)')
      : undefined) as MediaQueryList | undefined,
    lastFocusRevalidation: Number.NEGATIVE_INFINITY,
    navigation: undefined as unknown as NavigationController,
    syncedBrowsing: undefined as unknown as SyncedBrowsingController | undefined,
    settledRefreshTimers: new Map<PaneId, ReturnType<typeof setTimeout>>(),
    osFileClipboard: createOsFileClipboard(() => client),
    rawOpsController: undefined as unknown as OperationsController,
    opsController: undefined as unknown as OperationsController,
    pendingOperationConfirmation: undefined as unknown as
      | {
          readonly request: OperationConfirmationRequest;
          readonly resolve: (decision: OperationConfirmationDecision) => void;
        }
      | undefined,
    pendingPermanentDelete: undefined as unknown as
      | { readonly sources: readonly Location[]; readonly resolve: (confirmed: boolean) => void }
      | undefined,
    workspaceController: undefined as unknown as WorkspaceController,
    tabController: undefined as unknown as TabController,
    settingsController: undefined as unknown as SettingsController,
    findFilesController: undefined as unknown as FindFilesController,
    comparisonController: undefined as unknown as ComparisonController,
    syncProfileController: undefined as unknown as SyncProfileController,
    syncScheduleTimer: undefined as unknown as ReturnType<typeof setInterval> | undefined,
    checksumController: undefined as unknown as ChecksumController,
    basketFolderScan: undefined as unknown as AbortController | undefined,
    actionCommandController: undefined as unknown as ActionCommandController,
    renameRequests: new Map<PaneId, PaneRenameRequest>(),
    locationEditRequests: new Map<PaneId, number>(),
    paneContentBuilder: undefined as unknown as (
      client: FileManagerClient,
      entryFormatSettings: EntryFormatSettings,
      paneId: PaneId,
    ) => WorkspacePaneContent,
  };
}

/** The shell's state shape, inferred from {@link createShellState}. */
export type ShellState = ReturnType<typeof createShellState>;
