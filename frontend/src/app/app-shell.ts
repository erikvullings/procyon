import { Channel, invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import m, { type FactoryComponent } from 'mithril';
import { IconButton, ModalPanel, ThemeManager, toast } from 'mithril-materialized';

import packageJson from '../../package.json' with { type: 'json' };

import type { FileManagerClient } from '../api/client/file-manager-client';
import { ApiError } from '../api/fetch-mutator';
import {
  activityIcon,
  arrowLeftIcon,
  arrowRightIcon,
  basketIcon,
  basketOpenIcon,
  closeIcon,
  commandIcon,
  compareIcon,
  contentSearchIcon,
  cornerLeftUpIcon,
  helpIcon,
  layoutGridIcon,
  listIcon,
  plusIcon,
  searchIcon,
  settingsIcon,
  themeMapIcon,
} from '../components/tabler-icons';
import { tooltip } from '../components/tooltip';
import {
  type ActionCommandControllerContext,
  createActionCommandController,
} from '../features/actions/action-command-controller';
import { ChangeAttributesDialog } from '../features/attributes/change-attributes-dialog';
import {
  type ChecksumControllerContext,
  createChecksumController,
} from '../features/checksums/checksum-controller';
import { ChecksumResultsView } from '../features/checksums/checksum-results-view';
import { totalReclaimableBytes, wouldDeleteEveryCopy } from '../features/checksums/checksum-state';
import { DuplicateReviewView } from '../features/checksums/duplicate-review-view';
import { CommandPalette } from '../features/command-palette/command-palette';
import {
  evaluateActionAvailability,
  menuActionsForContext,
} from '../features/commands/availability';
import { ContextMenu as DirectoryContextMenu } from '../features/commands/context-menu';
import {
  type ComparisonControllerContext,
  createComparisonController,
} from '../features/comparison/comparison-controller';
import { differingEntryIds } from '../features/comparison/comparison-state';
import { SyncPlanDialog } from '../features/comparison/sync-plan-dialog';
import { type AppDialogsContext, renderAppDialogs } from '../features/dialogs/app-dialogs';
import { DirectoryTree, type DirectoryTreeAttrs } from '../features/directory-tree/directory-tree';
import {
  type DropModifiers,
  operationForDrop,
  validateDropTarget,
} from '../features/drag-drop/drag-drop';
import type { EntryFormatSettings } from '../features/entry-formatting/entry-formatting';
import {
  type BackendEventContext,
  createBackendEventHandler,
} from '../features/events/backend-event-handler';
import { recordRecentLocation } from '../features/favourites/favourites';
import { FileDiffDialog } from '../features/file-diff/file-diff-dialog';
import { ConvertGeoDialog } from '../features/geo/convert-geo-dialog';
import { MinifyGeoDialog } from '../features/geo/minify-geo-dialog';
import { mapThumbnailStats } from '../features/image-map/thumbnail-image';
import { EditImageMetadataDialog } from '../features/image-metadata/edit-image-metadata-dialog';
import {
  createGlobalKeydownHandler,
  type GlobalKeydownContext,
} from '../features/keybindings/global-keydown-handler';
import { ShortcutsHelpDialog } from '../features/keybindings/shortcuts-help-dialog';
import { createLinkDoctorController } from '../features/link-doctor/link-doctor-controller';
import { LinkDoctorView } from '../features/link-doctor/link-doctor-view';
import { createMusicDuplicatesController } from '../features/music-duplicates/music-duplicates-controller';
import { MusicDuplicatesView } from '../features/music-duplicates/music-duplicates-view';
import { replayEditCommand } from '../features/native-menu/edit-command';
import {
  dispatchNativeMenuAction,
  type NativeMenuDispatchContext,
} from '../features/native-menu/native-menu-dispatch';
import { buildNativeMenuSpec, type NativeMenuTab } from '../features/native-menu/native-menu-spec';
import { WindowsNativeMenu } from '../features/native-menu/windows-native-menu';
import {
  createNavigationController,
  type NavigationController,
  parentLocation,
} from '../features/navigation/navigation';
import {
  createLocalStorageSyncStore,
  createSyncedBrowsing,
} from '../features/navigation/synced-browsing';
import {
  requestSkippingSpaceCheck,
  spaceShortfallOf,
} from '../features/operations/insufficient-space';
import { InsufficientSpaceDialog } from '../features/operations/insufficient-space-dialog';
import { itemProgressSummary, operationKindLabel } from '../features/operations/operation-centre';
import {
  createOperationsState,
  dismissOperation,
  isActiveOperation,
  shouldAutoDismissOperation,
  summariseActiveOperations,
} from '../features/operations/operation-state';
import {
  createOperationsController,
  type OperationConfirmationDecision,
  withActiveSourceGuard,
  withOperationConfirmation,
  withPermanentDeleteConfirmation,
} from '../features/operations/operations-controller';
import { isParentEntry } from '../features/panes/parent-entry';
import { createTabController, type TabControllerContext } from '../features/panes/tab-controller';
import { createMediaAccess, type PrepareMedia } from '../features/player/media-access';
import { MiniPlayer } from '../features/player/mini-player';
import { createPlayerController } from '../features/player/player-controller';
import type { PlayerState } from '../features/player/player-state';
import { SavePlaylistDialog } from '../features/playlist/save-playlist-dialog';
import { filterEntriesAdvanced } from '../features/quick-filter/advanced-filter';
import { filterEntries } from '../features/quick-filter/quick-filter';
import { collectRecursiveQuickFilterEntries } from '../features/quick-filter/recursive-quick-filter';
import {
  createFindFilesController,
  type FindFilesControllerContext,
} from '../features/search/find-files-controller';
import {
  emptySelection,
  getSelectedEntries,
  getSelectedEntriesOrCursor,
  reduceSelection,
} from '../features/selection/selection';
import {
  pdfImageProvenance,
  pdfPageRenderProvenance,
} from '../features/semantic/evidence-provenance';
import { AppUpdateDialog } from '../features/settings/app-update-dialog';
import { SemanticFolderEnrolmentPrompt } from '../features/settings/semantic-library-management';
import {
  createSettingsController,
  type SettingsControllerContext,
} from '../features/settings/settings-controller';
import { SettingsEditor } from '../features/settings/settings-editor';
import { SaveSnapshotDialog } from '../features/snapshots/save-snapshot-dialog';
import { createSnapshotController } from '../features/snapshots/snapshot-controller';
import { childLocation } from '../features/snapshots/snapshot-state';
import { SnapshotView } from '../features/snapshots/snapshot-view';
import {
  type SortColumn,
  type SortModel,
  sortEntries,
  sortEntriesResponsive,
} from '../features/sorting/sorting';
import { CombineFilesDialog } from '../features/split/combine-files-dialog';
import { SplitFileDialog } from '../features/split/split-file-dialog';
import { SYNC_SCHEDULE_TICK_MS } from '../features/sync-profiles/sync-profile-actions';
import {
  createSyncProfileController,
  type SyncProfileControllerContext,
} from '../features/sync-profiles/sync-profile-controller';
import { SyncProfilesDialog } from '../features/sync-profiles/sync-profiles-dialog';
import { tauriTerminalClient } from '../features/terminal/terminal-client';
import { TerminalDrawer } from '../features/terminal/terminal-drawer';
import { focusOpenedTerminal, isTerminalVisible } from '../features/terminal/terminal-state';
import { WalkthroughPanel } from '../features/walkthroughs/walkthrough-panel';
import { formatWindowTitle } from '../features/window-title/window-title';
import { dispatchWorkspaceCommand } from '../features/workspace/dispatch-workspace-command';
import {
  createPaneContentBuilder,
  type PaneContentContext,
} from '../features/workspace/pane-content-builder';
import {
  createWorkspaceController,
  type WorkspaceControllerContext,
} from '../features/workspace/workspace-controller';
import { WorkspaceLayoutView } from '../features/workspace/workspace-layout';
import { sortWorkspaceSummaries } from '../features/workspace/workspace-manager';
import { WorkspaceSwitcher } from '../features/workspace/workspace-switcher';
import { errorText, t } from '../i18n';
import { footerFunctionKeyBindings, getLiveBindings } from '../keybindings/dispatcher';
import type {
  ActionInvocationContext,
  DirectoryDelta,
  EntrySummary,
  Location,
  NativeMenuSpec,
  Operation,
  PaneId,
  PluginId,
  PluginLogEntry,
  SemanticComponentStatus,
  Settings,
  SortDescriptor,
  TabId,
  TabProjection,
  WorkspaceId,
  WorkspaceLayout,
  WorkspaceProjection,
} from '../models';
import {
  applyAppPatches,
  cacheContentMatchesPatch,
  connectionPatch,
  createInitialAppState,
  deleteQuickFilterDraftPatch,
  playerPatch,
  setQuickFilterDraftPatch,
} from '../state';
import {
  hasPerfSink,
  perfEndAfterPaint,
  perfEndAfterRender,
  perfStart,
  perfStat,
} from '../utilities/perf';
import type { RuntimeKind } from '../utilities/runtime';
import { createWindowTitleSink, dedupeWindowTitleSink } from '../utilities/window-title-sink';
import { createActionsWiring } from './actions-wiring';
import { createBasketWiring } from './basket-wiring';
import { buildControllers } from './controller-registry';
import { createDialogWiring } from './dialogs-wiring';
import { createImageMapWiring } from './image-map-wiring';
import { createKnowledgeWiring } from './knowledge-wiring';
import { createMiscWiring } from './misc-wiring';
import { createOperationsWiring } from './operations-wiring';
import { createPreviewWiring } from './preview-wiring';
import { createSettingsWiring } from './settings-wiring';
import { locationForPath, respectSystemLocationReadOnly, windowControlIcon } from './shell-helpers';
import type { DiskUsageTabEntry, KnowledgeSearchTabEntry, ThemesTabEntry } from './shell-state';
import { createShellState } from './shell-state';
import { createTreeWiring } from './tree-wiring';

export { knowledgeEvidencePage } from '../features/semantic/evidence-provenance';
export {
  locationForPath,
  removeDiskUsageNodes,
  respectSystemLocationReadOnly,
} from './shell-helpers';

/** Attributes of the application shell. */
export interface AppShellAttrs {
  /** Transport this build talks to, resolved from `VITE_RUNTIME`. */
  runtime: RuntimeKind;
  /** Transport-neutral client selected once by the application bootstrap. */
  client: FileManagerClient;
  /** Settings-owned presentation formats; task 0030 supplies these at bootstrap. */
  entryFormatSettings?: EntryFormatSettings;
}

export const AppShell: FactoryComponent<AppShellAttrs> = (vnode) => {
  const shellState = createShellState(vnode.attrs.client);
  /** Unlocks encrypted archive entries before they are streamed (task 0269). */
  const prepareMedia: PrepareMedia = (location, signal) =>
    createMediaAccess({ client: shellState.client, requestArchivePassword })(location, signal);
  let opsWiring: ReturnType<typeof createOperationsWiring>;
  let basketWiring: ReturnType<typeof createBasketWiring>;
  let imageMapWiring: ReturnType<typeof createImageMapWiring>;
  let dialogWiring: ReturnType<typeof createDialogWiring>;
  let actionsWiring: ReturnType<typeof createActionsWiring>;
  let previewWiring: ReturnType<typeof createPreviewWiring>;
  let knowledgeWiring: ReturnType<typeof createKnowledgeWiring>;
  let treeWiring: ReturnType<typeof createTreeWiring>;
  let settingsWiring: ReturnType<typeof createSettingsWiring>;
  let miscWiring: ReturnType<typeof createMiscWiring>;
  const core = {
    activeTabKey,
    entriesSortedFor,
    entriesFilteredFor,
    quickFilterQueryFor,
    activeDirectory,
    selectTab,
    activatePane,
    replaceWorkspace,
    refetchAffectedPanes,
    prepareMedia,
    openViewer: (
      client: FileManagerClient,
      paneId: PaneId,
      entry: EntrySummary,
      initialSearch?: {
        readonly query: string;
        readonly regex: boolean;
        readonly caseSensitive: boolean;
        readonly wholeWord: boolean;
      },
      openMetadata?: boolean,
      initialPage?: number,
      knowledgeSourceId?: string,
    ) =>
      previewWiring.openViewer(
        client,
        paneId,
        entry,
        initialSearch,
        openMetadata,
        initialPage,
        knowledgeSourceId,
      ),
    openEditor: (
      client: FileManagerClient,
      paneId: PaneId,
      entry: EntrySummary,
      initialLine?: number,
    ) => previewWiring.openEditor(client, paneId, entry, initialLine),
    actOnLinkAtCursor: (mode: 'open' | 'reveal') => previewWiring.actOnLinkAtCursor(mode),
    beginShowOnMap: (invocation: ActionInvocationContext | undefined) =>
      imageMapWiring.beginShowOnMap(invocation),
    refreshSemanticAssistantAvailability: (currentComponents?: SemanticComponentStatus) =>
      knowledgeWiring.refreshSemanticAssistantAvailability(currentComponents),
    refreshKnowledgeSearchAvailability: () => knowledgeWiring.refreshKnowledgeSearchAvailability(),
  };
  opsWiring = createOperationsWiring(shellState, core);
  basketWiring = createBasketWiring(shellState, core);
  imageMapWiring = createImageMapWiring(shellState, core);
  dialogWiring = createDialogWiring(shellState, core);
  actionsWiring = createActionsWiring(shellState);
  previewWiring = createPreviewWiring(shellState, core);
  knowledgeWiring = createKnowledgeWiring(shellState, core);
  treeWiring = createTreeWiring(shellState, core);
  settingsWiring = createSettingsWiring(shellState, core);
  miscWiring = createMiscWiring(shellState, core);
  function closeActivityOnOutsidePointer(event: PointerEvent): void {
    if (!shellState.activityOpen || (event.target as Element).closest('.fm-activity-anchor'))
      return;
    shellState.activityOpen = false;
    m.redraw();
  }

  /** Opens the archive password prompt; resolves `false` when the user cancels. */
  function requestArchivePassword(location: Location, invalid: boolean): Promise<boolean> {
    shellState.dialogs.getState().pendingArchiveCredential?.resolve(false);
    shellState.dialogs.clearArchiveCredential();
    return new Promise<boolean>((resolve) => {
      shellState.dialogs.setPendingArchiveCredential({ location, invalid, resolve });
      m.redraw();
    });
  }

  /** Every open tab across every pane, flattened for the native Window menu (task 0133). */
  function nativeMenuWindowTabs(): readonly NativeMenuTab[] {
    if (shellState.workspace === undefined) return [];
    const tabs: NativeMenuTab[] = [];
    for (const paneId of shellState.workspace.paneOrder) {
      const pane = shellState.workspace.panesById[paneId];
      if (pane === undefined) continue;
      for (const tabId of pane.tabOrder) {
        const projection = pane.tabsById[tabId];
        if (projection === undefined) continue;
        tabs.push({
          paneId,
          tabId,
          tabKey: tabKey(paneId, tabId),
          title: projection.title,
          active: pane.activeTabId === tabId,
        });
      }
    }
    return tabs;
  }

  function syncWindowTitle(): void {
    shellState.windowTitleSink(formatWindowTitle({ workspace: shellState.workspace }));
  }

  function syncNativeMenu(): void {
    if (shellState.runtimeKind !== 'tauri') return;
    const spec: NativeMenuSpec = buildNativeMenuSpec({
      // Client-only actions (Search Knowledge, disk usage, ...) have no backend
      // registry entry, so the menu builder can only find them when they are
      // passed alongside the registered ones - exactly as the command palette
      // and the menu dispatch context already do.
      actions: [
        ...actionsWiring.localisedRegisteredActions(),
        ...actionsWiring.clientOnlyActions(),
      ],
      favouriteActions: actionsWiring.favouriteActions(),
      tabs: nativeMenuWindowTabs(),
      canOpenNewWindow: shellState.client.openWorkspaceWindow !== undefined,
      macEditCommands: shellState.platform === 'macos',
      workspaces: sortWorkspaceSummaries(shellState.workspaceSummaries),
      currentWorkspaceId: shellState.workspace?.id,
      volumes: shellState.volumes,
      connections: shellState.connections,
      systemLocations: shellState.systemLocations,
      unavailableLocations: shellState.unavailableLocations,
    });
    shellState.windowsNativeMenuSpec = spec;
    if (isWindowsTauriHost() || !shellState.nativeMenuChannelReady) return;
    const serialized = JSON.stringify(spec);
    if (serialized === shellState.lastSentNativeMenuSpecJson) return;
    shellState.lastSentNativeMenuSpecJson = serialized;
    void invoke('set_native_menu', { spec }).catch(() => {
      // The native menu bar is cosmetic desktop chrome; a failed push shouldn't surface an error.
    });
  }

  const nativeMenuDispatchContext: NativeMenuDispatchContext = {
    findAction: (id) =>
      actionsWiring.actionsWithFavourites().find((candidate) => candidate.id === id),
    openSettingsDialog: () => {
      settingsWiring.openSettingsDialog();
    },
    openDiagnostics: () => {
      settingsWiring.openSettingsDialog('diagnostics');
    },
    openShortcutsHelp: () => {
      shellState.shortcutsHelpOpen = true;
      m.redraw();
    },
    openWalkthroughs: () => {
      shellState.walkthroughsOpen = true;
      m.redraw();
    },
    activateTabByKey: (key) => {
      const separator = key.indexOf(':');
      if (separator < 0) return;
      const paneId = key.slice(0, separator);
      const tabId = key.slice(separator + 1);
      if (shellState.workspace?.panesById[paneId]?.activeTabId === tabId) {
        // The Window menu lists every open tab per pane, so clicking one that's already its
        // pane's active/displayed tab is a normal, common case: switching keyboard focus to
        // another pane without changing what it shows - there's no equivalent single click for
        // this in the tab bar (you'd click the pane's content area instead). Routing it through
        // tabController.activateTab would hit its "re-click the same tab" branch, which triggers
        // a background reload without ever updating which pane has focus and can disturb that
        // pane's existing selection. activatePane is the lightweight, no-reload "just switch
        // focus" operation this needs instead.
        void activatePane(shellState.client, paneId).catch(() => undefined);
        return;
      }
      shellState.tabController.activateTab(paneId, tabId);
    },
    activePaneId: () => activeDirectory()?.paneId,
    openNewTab: (paneId) => shellState.tabController.openTab(paneId),
    closeActiveTab: (paneId) => {
      const focusedEditorPaneId = (
        document.activeElement?.closest('.fm-file-editor') as HTMLElement | null
      )?.dataset.editorPaneId as PaneId | undefined;
      if (
        focusedEditorPaneId !== undefined &&
        previewWiring.requestCloseEditor(focusedEditorPaneId)
      )
        return;
      const activeTabId = shellState.workspace?.panesById[paneId]?.activeTabId;
      if (activeTabId !== undefined) shellState.tabController.requestCloseTab(paneId, activeTabId);
    },
    setSort: (paneId, sort) => globalKeydownHandlerContext.setSort(paneId, sort),
    invokeAction: (action) => {
      const actionContext = shellState.actionCommandController.actionContext();
      if (
        (action.id === 'core.copyName' ||
          action.id === 'core.copyPath' ||
          action.id === 'core.copyRelativePath') &&
        actionContext.selectedEntryIds === undefined
      ) {
        const active = activeDirectory();
        const cursorEntryId =
          active === undefined
            ? undefined
            : shellState.selections.get(activeTabKey(active.paneId))?.cursorEntryId;
        if (cursorEntryId !== undefined) {
          shellState.actionCommandController.invokePaletteAction(action, undefined, {
            ...actionContext,
            selectedEntryIds: [cursorEntryId],
          });
          return;
        }
      }
      shellState.actionCommandController.invokePaletteAction(action, undefined, actionContext);
    },
    openNewWorkspaceWindow: () => {
      if (shellState.workspace === undefined) return;
      // An ephemeral window forks from the named workspace its own session came from - not from
      // `workspace.id` itself, which is this window's own (possibly unsynced) ephemeral copy.
      // `forkedFrom` is `undefined` for a from-scratch default session, meaning the new window
      // seeds from the hardcoded default too, rather than forking a fork of a fork. A
      // non-ephemeral window (the main/dock window, which always loads the last-active named
      // workspace via `start_workspace`) IS itself the named workspace, so it forks from its own
      // id - otherwise "New Window" from the dock always fell back to the hardcoded default.
      void shellState.client.openWorkspaceWindow?.(
        shellState.workspace.ephemeral ? shellState.workspace.forkedFrom : shellState.workspace.id,
      );
    },
    openWorkspaceWindowById: (workspaceId) => {
      void shellState.client.openWorkspaceWindow?.(workspaceId);
    },
    resyncWorkspace: () => {
      void resyncWorkspace(shellState.client);
    },
    getVolumes: () => shellState.volumes,
    getConnections: () => shellState.connections,
    getSystemLocations: () => shellState.systemLocations,
    navigateToLocation: (location) => {
      const paneId = activeDirectory()?.paneId;
      if (paneId === undefined) return;
      void shellState.navigation.navigate(paneId, location);
    },
    performEditCommand: (command) => {
      replayEditCommand(command, document.activeElement, (unhandled) => {
        void invoke('perform_native_edit_command', { command: unhandled }).catch(() => undefined);
      });
    },
  };
  const isWindowsTauriHost = (): boolean =>
    shellState.runtimeKind === 'tauri' && /Windows/i.test(navigator.userAgent);
  /** Manual drag start (Tauri's "Manual Implementation of data-tauri-drag-region" pattern)
   * instead of the declarative attribute: the declarative form left WebView2's mouse/pointer
   * capture in a bad state after a drag, so titlebar menu clicks stopped registering until a
   * second click. Starting the OS drag ourselves on primary-button mousedown (and toggling
   * maximize on double-click, standard titlebar UX) avoids that stuck capture. */
  function startWindowTitlebarDrag(event: MouseEvent): void {
    if (event.buttons !== 1) return;
    event.preventDefault();
    if (event.detail === 2) {
      void getCurrentWindow().toggleMaximize();
      return;
    }
    void getCurrentWindow().startDragging();
  }

  const DEFAULT_SORT: readonly SortDescriptor[] = [
    { columnId: 'core.name', direction: 'ascending' },
  ];

  /** Minimum gap between focus-triggered revalidations (focus can fire in bursts). */
  const FOCUS_REVALIDATE_MIN_INTERVAL_MS = 2_000;

  /**
   * Safety net behind the filesystem watchers: events can be missed (coalesced FSEvents, a watcher
   * that died, a stream that lagged), and the user returning to the window is the moment they would
   * notice a stale folder. A background reload is cheap and publishes nothing visible if unchanged.
   */
  function revalidateDirectoriesOnReturn(): void {
    const now = Date.now();
    if (now - shellState.lastFocusRevalidation < FOCUS_REVALIDATE_MIN_INTERVAL_MS) return;
    shellState.lastFocusRevalidation = now;
    refetchAffectedPanes(undefined, { background: true });
  }

  /** Restores real DOM keyboard focus to the active pane whenever the OS window regains it (e.g.
   * alt-tabbing back into the app). Without this, `document.activeElement` is left wherever it was
   * before the app lost focus (often nowhere useful), so the cursor row still *looks* highlighted
   * but arrow keys silently do nothing until the user clicks a row to re-establish focus manually. */
  function handleWindowFocus(): void {
    void shellState.workspaceController.refreshVolumes();
    const openModals = document.querySelectorAll<HTMLElement>('.modal.active[aria-modal="true"]');
    const openModal = openModals.item(openModals.length - 1);
    if (openModal !== null) {
      if (!openModal.contains(document.activeElement)) {
        const defaultAction =
          openModal.querySelector<HTMLElement>('.mm-dialog-primary-action:not([disabled])') ??
          openModal.querySelector<HTMLElement>('.fm-permanent-delete-confirm:not([disabled])');
        const firstControl = openModal.querySelector<HTMLElement>(
          'button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled])',
        );
        (defaultAction ?? firstControl ?? openModal).focus();
      }
      return;
    }
    revalidateDirectoriesOnReturn();
    const activePaneId = shellState.workspace?.activePaneId;
    if (activePaneId === undefined) return;
    if (shellState.focusPane !== undefined) shellState.focusPane(activePaneId);
    else void activatePane(shellState.client, activePaneId);
  }

  /** Periodic cache-size statistics for memory diagnostics (desktop log, `stat-cache` entries). */
  function reportCacheStats(): void {
    if (!hasPerfSink() || document.hidden) return;
    const sum = (arrays: Iterable<{ readonly length: number }>): number => {
      let total = 0;
      for (const array of arrays) total += array.length;
      return total;
    };
    const thumbnails = shellState.thumbnailLoader?.stats();
    const icons = shellState.nativeIconLoader?.stats();
    const mapThumbnails = mapThumbnailStats();
    perfStat('cache', {
      directorySessions: shellState.directories.size,
      directoryEntries: sum([...shellState.directories.values()].map((view) => view.entries)),
      sortedCopies: shellState.sortedEntries.size,
      sortedEntries: sum([...shellState.sortedEntries.values()].map((cached) => cached.entries)),
      filteredCopies: shellState.filteredEntries.size,
      filteredEntries: sum(
        [...shellState.filteredEntries.values()].map((cached) => cached.entries),
      ),
      viewers: shellState.viewerByTab.size,
      thumbnails: thumbnails?.count ?? 0,
      thumbnailMChars: Math.round((thumbnails?.chars ?? 0) / 1_000_000),
      icons: icons?.count ?? 0,
      iconKChars: Math.round((icons?.chars ?? 0) / 1_000),
      mapThumbnails: mapThumbnails.count,
      mapThumbnailMChars: Math.round(mapThumbnails.chars / 1_000_000),
    });
  }

  function handleVisibilityChange(): void {
    if (document.visibilityState === 'visible') {
      revalidateDirectoriesOnReturn();
      void shellState.workspaceController.refreshVolumes();
    }
  }

  function effectiveSort(sort: readonly SortDescriptor[]): readonly SortDescriptor[] {
    return sort.length === 0 ? DEFAULT_SORT : sort;
  }

  /** Composes the per-tab cache key every runtime-state Map above is keyed by. */
  function tabKey(paneId: PaneId, tabId: TabId): string {
    return `${paneId}:${tabId}`;
  }

  /** The composite key for whichever tab is currently active in `paneId`. */
  function activeTabKey(paneId: PaneId): string {
    const pane = shellState.workspace?.panesById[paneId];
    return tabKey(paneId, pane?.activeTabId ?? '');
  }

  function frontendSort(sort: readonly SortDescriptor[]): SortModel {
    const descriptor = sort[0];
    if (descriptor === undefined) return [];
    const columns: Readonly<Record<string, SortColumn>> = {
      'core.name': 'name',
      'core.extension': 'extension',
      'core.size': 'size',
      'core.modified': 'modified',
      'sample.fileAge': 'modified',
    };
    const column = columns[descriptor.columnId];
    return column === undefined ? [] : [{ column, direction: descriptor.direction }];
  }

  function sortLabel(sort: readonly SortDescriptor[]): string {
    const descriptor = sort[0];
    if (descriptor === undefined) return t('shell', 'unsorted');
    const labels: Readonly<Record<string, string>> = {
      'core.name': t('table', 'name'),
      'core.extension': t('pane', 'extension'),
      'core.size': t('table', 'size'),
      'core.modified': t('table', 'modified'),
      'sample.fileAge': t('table', 'age'),
    };
    return `${labels[descriptor.columnId] ?? descriptor.columnId} ${descriptor.direction}`;
  }

  function entriesSortedFor(
    key: string,
    entries: readonly EntrySummary[],
    sort: readonly SortDescriptor[],
    foldersFirst: boolean,
    groupByParentPath = false,
  ): readonly EntrySummary[] {
    const cacheKey = JSON.stringify([sort, foldersFirst, groupByParentPath]);
    const cached = shellState.sortedEntries.get(key);
    if (cached?.input === entries && cached.key === cacheKey) {
      return cached.entries;
    }
    const model = frontendSort(sort);
    if (entries.length < 10_000) {
      const sorted = sortEntries(entries, model, foldersFirst, { groupByParentPath });
      shellState.sortedEntries.set(key, { input: entries, key: cacheKey, entries: sorted });
      return sorted;
    }
    const request = {};
    shellState.sortRequests.set(key, request);
    void sortEntriesResponsive(entries, model, foldersFirst, { groupByParentPath }).then(
      (sorted) => {
        if (shellState.sortRequests.get(key) === request) {
          shellState.sortedEntries.set(key, { input: entries, key: cacheKey, entries: sorted });
          shellState.sortRequests.delete(key);
          m.redraw();
        }
      },
    );
    return cached?.entries ?? entries;
  }

  function entriesFilteredFor(
    key: string,
    entries: readonly EntrySummary[],
    query: string,
  ): readonly EntrySummary[] {
    const criteria = shellState.advancedFilters.criteriaFor(key);
    // Task 0310: when the recursive "this folder + subfolders" scope is on and a query is present,
    // filter the walk's collected results instead of just the current folder's entries.
    const source =
      quickFilterRecursiveFor(key) && query.trim() !== ''
        ? (recursiveQuickFilterResultsFor(key) ?? entries)
        : entries;
    const cached = shellState.filteredEntries.get(key);
    if (cached?.input === source && cached.query === query && cached.criteria === criteria) {
      return cached.entries;
    }
    const filtered = filterEntriesAdvanced(filterEntries(source, query), criteria);
    shellState.filteredEntries.set(key, { input: source, query, criteria, entries: filtered });
    return filtered;
  }

  function quickFilterQueryFor(key: string, tab: TabProjection | undefined): string {
    return (
      shellState.appState?.quickFilterDrafts.byTabKey[key] ?? tab?.view.quickFilter?.query ?? ''
    );
  }

  function quickFilterOpenFor(key: string, tab: TabProjection | undefined): boolean {
    return (
      shellState.quickFilterOpen.get(key) === true ||
      (tab?.view.quickFilter ?? null) !== null ||
      shellState.advancedFilters.isActive(key)
    );
  }

  /** Task 0310: is the recursive "this folder + subfolders" quick-filter scope on for this tab? */
  function quickFilterRecursiveFor(key: string): boolean {
    return shellState.quickFilterRecursive.get(key)?.recursive === true;
  }

  function setQuickFilterRecursive(key: string, recursive: boolean): void {
    const state = shellState.quickFilterRecursive.get(key) ?? {
      recursive: false,
      controller: undefined,
      results: undefined,
      base: [],
    };
    state.recursive = recursive;
    shellState.quickFilterRecursive.set(key, state);
  }

  function recursiveQuickFilterResultsFor(key: string): readonly EntrySummary[] | undefined {
    return shellState.quickFilterRecursive.get(key)?.results;
  }

  /** Task 0310: (re)run the bounded recursive walk for `key`'s current query over `baseEntries`.
   * A no-op for tabs that have never toggled the recursive scope on. */
  function runRecursiveQuickFilter(
    key: string,
    query: string,
    baseEntries: readonly EntrySummary[],
  ): void {
    const state = shellState.quickFilterRecursive.get(key);
    if (state === undefined) return;
    state.base = baseEntries;
    state.controller?.abort();
    if (!state.recursive || query.trim() === '') {
      state.results = undefined;
      m.redraw();
      return;
    }
    const controller = new AbortController();
    state.controller = controller;
    state.results = undefined;
    const showHidden = shellState.currentSettings?.showHiddenFiles === true;
    void collectRecursiveQuickFilterEntries(
      baseEntries,
      query,
      (location, signal) => vnode.attrs.client.listDirectoryChildren(location, showHidden, signal),
      { signal: controller.signal },
    )
      .then((results) => {
        if (controller.signal.aborted) return;
        state.results = results;
        m.redraw();
      })
      .catch(() => undefined);
  }

  /** Task 0310: flip the recursive scope for a tab and immediately re-search the current query. */
  function toggleQuickFilterScope(key: string): void {
    const next = !quickFilterRecursiveFor(key);
    setQuickFilterRecursive(key, next);
    const state = shellState.quickFilterRecursive.get(key);
    if (next) {
      const draft = shellState.appState?.quickFilterDrafts.byTabKey[key] ?? '';
      runRecursiveQuickFilter(key, draft, state?.base ?? []);
    } else {
      state?.controller?.abort();
      if (state) state.results = undefined;
    }
    m.redraw();
  }

  /** If `entry` came from a content-search results tab (`locationUri`) and has content matches,
   * returns the original content-search query so the viewer can pre-populate and highlight it
   * (task 0089 follow-up) - otherwise `undefined`. Shared by both the double-click/Enter open
   * path and the F3 view shortcut, so pressing either while a search result is selected jumps
   * straight to the match instead of opening a blank/unsearched viewer. */
  function contentSearchInitialQuery(
    locationUri: string,
    entry: EntrySummary,
  ):
    | {
        readonly query: string;
        readonly regex: boolean;
        readonly caseSensitive: boolean;
        readonly wholeWord: boolean;
      }
    | undefined {
    const params = shellState.findFilesParamsByLocationUri.get(locationUri);
    if (params?.contentQuery === undefined || params.contentQuery === '') return undefined;
    // A directory listing refetched via REST (`navigation.load()`, e.g. after a subsequent
    // search batch or a plain tab switch) never carries `contentMatches` - only the live
    // `search.resultsBatch` SSE event does (`EntrySummaryDto` has no such field) - so fall back
    // to whatever that event most recently cached for this entry's location.
    const matches =
      entry.contentMatches ?? shellState.appState?.contentMatches.byEntryUri[entry.location.uri];
    if (matches === undefined || matches.length === 0) return undefined;
    return {
      query: params.contentQuery,
      regex: params.contentRegex,
      caseSensitive: false,
      wholeWord: true,
    };
  }

  function pdfImagePageForEntry(locationUri: string, entry: EntrySummary): number | undefined {
    if (!locationUri.startsWith('search://') || entry.mimeType !== 'application/pdf') {
      return undefined;
    }
    const result = shellState.findFilesPresentationsByLocationUri
      .get(locationUri)
      ?.semanticResults?.find((candidate) => candidate.entryId === entry.id);
    const evidence = result?.bestEvidence;
    return evidence?.chunkKind === 'pdf_image' && evidence.available && !evidence.stale
      ? (
          pdfImageProvenance(evidence.provenanceJson) ??
          pdfPageRenderProvenance(evidence.provenanceJson)
        )?.page
      : undefined;
  }

  /** Recursively sums `entry`'s size (task 0071's Total Commander-style folder-size key, Ctrl+.)
   * and patches its row's `size` field locally once the walk completes - no backend event/delta is
   * involved, so a result that arrives after the user has navigated elsewhere (the entry no longer
   * present in `paneId`'s current listing) is silently discarded rather than misapplied. Only the
   * most recently started calculation is kept - starting a new one implicitly abandons any previous
   * still in flight (mirrors the single-viewer-at-a-time convention elsewhere in this file). */
  function calculateFolderSize(
    client: FileManagerClient,
    paneId: PaneId,
    entry: EntrySummary,
  ): void {
    shellState.folderSizeCalculation?.abort();
    const controller = new AbortController();
    shellState.folderSizeCalculation = controller;
    void client
      .calculateFolderSize({ location: entry.location }, controller.signal)
      .then((result) => {
        if (controller.signal.aborted) return;
        const key = activeTabKey(paneId);
        const current = shellState.directories.get(key);
        if (current === undefined) return;
        shellState.directories.set(key, {
          ...current,
          entries: current.entries.map((candidate) =>
            candidate.id === entry.id ? { ...candidate, size: result.totalBytes } : candidate,
          ),
        });
        m.redraw();
      })
      .catch((error: unknown) => {
        if (controller.signal.aborted) return;
        toast({
          html: t('shell', 'folderSizeError', {
            name: entry.name,
            error: errorText(error),
          }),
        });
      });
  }

  /** Scans `entry`'s well-known related-file locations (task 0148's macOS uninstaller) and opens
   * the review checklist once discovery completes - discovery itself never deletes anything;
   * deletion only happens if the user confirms the dialog. A discovery failure surfaces as a
   * toast, mirroring {@link calculateFolderSize}'s error handling. */
  function uninstallApplication(
    client: FileManagerClient,
    _paneId: PaneId,
    entry: EntrySummary,
  ): void {
    void client
      .discoverApplicationUninstallCandidates({ location: entry.location })
      .then((result) => {
        shellState.dialogs.openApplicationUninstallDialog({
          bundle: entry,
          productName: result.productName,
          relatedFiles: result.relatedFiles,
        });
        m.redraw();
      })
      .catch((error: unknown) => {
        toast({
          html: t('applicationUninstall', 'discoveryError', {
            name: entry.name,
            error: errorText(error),
          }),
        });
      });
  }

  /** Finds the directory `name` directly inside `parent`, matching case-insensitively (task 0255). */
  async function resolveSyncChild(parent: Location, name: string): Promise<Location | undefined> {
    const wanted = name.toLowerCase();
    const children = await shellState.client.listDirectoryChildren(parent, true);
    const exact = children.find((child) => child.name === name);
    return (exact ?? children.find((child) => child.name.toLowerCase() === wanted))?.location;
  }

  /** Mirrors user navigation between the panes while synchronized browsing is on (task 0255). */
  function wrapSyncedBrowsing(inner: NavigationController): NavigationController {
    shellState.syncedBrowsing = createSyncedBrowsing({
      navigation: inner,
      getWorkspace: () => shellState.workspace,
      resolveChild: resolveSyncChild,
      store: createLocalStorageSyncStore(window.localStorage),
      onChange: () => m.redraw(),
    });
    return shellState.syncedBrowsing.navigation;
  }

  /** Clears every per-tab runtime cache for a closed tab, cancelling its in-flight request. */
  function clearTabState(paneId: PaneId, tabId: TabId): void {
    if (shellState.pluginByPane.get(paneId)?.tabId === tabId)
      shellState.pluginByPane.delete(paneId);
    shellState.basketTabIds.delete(tabId);
    const key = tabKey(paneId, tabId);
    shellState.viewerByTab.get(key)?.controller.dispose();
    shellState.viewerByTab.delete(key);
    const diskUsage = shellState.diskUsageByTab.get(key);
    if (diskUsage !== undefined && !diskUsage.progressComplete) {
      diskUsage.abort.abort();
      void shellState.client.cancelDiskUsage(diskUsage.scanId).catch((error: unknown) => {
        console.warn('Failed to cancel disk-usage scan while closing its tab', error);
      });
    }
    shellState.diskUsageByTab.delete(key);
    if (shellState.imageMapTab?.paneId === paneId && shellState.imageMapTab.tabId === tabId) {
      shellState.imageMapTab = undefined;
      shellState.imageMapController?.close();
    }
    shellState.knowledgeSearchByTab.delete(key);
    shellState.themesByTab.delete(key);
    shellState.navigation.abort(paneId, tabId);
    shellState.directories.delete(key);
    shellState.selections.delete(key);
    shellState.pendingCursorRestorations.delete(key);
    shellState.sortedEntries.delete(key);
    shellState.sortRequests.delete(key);
    if (shellState.appState !== undefined)
      shellState.appState = applyAppPatches(shellState.appState, deleteQuickFilterDraftPatch(key));
    shellState.quickFilterOpen.delete(key);
    shellState.advancedFilters.clear(key);
    shellState.filteredEntries.delete(key);
    if (shellState.openTerminalTabKeys.delete(key)) shellState.disposeTerminalTab?.(key);
  }

  /** Preserves tab-owned UI state when the authoritative tab moves to another pane. */
  function moveTabState(sourcePaneId: PaneId, targetPaneId: PaneId, tabId: TabId): void {
    if (sourcePaneId === targetPaneId) return;
    if (shellState.pluginByPane.get(sourcePaneId)?.tabId === tabId)
      shellState.pluginByPane.delete(sourcePaneId);
    const sourceKey = tabKey(sourcePaneId, tabId);
    const targetKey = tabKey(targetPaneId, tabId);
    const rekey = <T>(values: Map<string, T>): void => {
      const value = values.get(sourceKey);
      if (value === undefined) return;
      values.delete(sourceKey);
      values.set(targetKey, value);
    };
    rekey(shellState.directories);
    rekey(shellState.selections);
    rekey(shellState.pendingCursorRestorations);
    rekey(shellState.sortedEntries);
    rekey(shellState.sortRequests);
    rekey(shellState.cursorLoadTokens);
    rekey(shellState.quickFilterOpen);
    shellState.advancedFilters.rekey(sourceKey, targetKey);
    rekey(shellState.filteredEntries);
    const viewer = shellState.viewerByTab.get(sourceKey);
    if (viewer !== undefined) {
      shellState.viewerByTab.delete(sourceKey);
      shellState.viewerByTab.set(targetKey, { ...viewer, paneId: targetPaneId });
    }
    const diskUsage = shellState.diskUsageByTab.get(sourceKey);
    if (diskUsage !== undefined) {
      shellState.diskUsageByTab.delete(sourceKey);
      diskUsage.key = targetKey;
      diskUsage.paneId = targetPaneId;
      shellState.diskUsageByTab.set(targetKey, diskUsage);
    }
    if (shellState.imageMapTab?.paneId === sourcePaneId && shellState.imageMapTab.tabId === tabId) {
      shellState.imageMapTab = { paneId: targetPaneId, tabId };
    }
    const knowledgeSearch = shellState.knowledgeSearchByTab.get(sourceKey);
    if (knowledgeSearch !== undefined) {
      shellState.knowledgeSearchByTab.delete(sourceKey);
      knowledgeSearch.key = targetKey;
      knowledgeSearch.paneId = targetPaneId;
      shellState.knowledgeSearchByTab.set(targetKey, knowledgeSearch);
    }
    const themesTab = shellState.themesByTab.get(sourceKey);
    if (themesTab !== undefined) {
      shellState.themesByTab.delete(sourceKey);
      themesTab.key = targetKey;
      themesTab.paneId = targetPaneId;
      shellState.themesByTab.set(targetKey, themesTab);
    }
    shellState.navigation.moveTab(sourcePaneId, targetPaneId, tabId);
    const quickFilterDraft = shellState.appState?.quickFilterDrafts.byTabKey[sourceKey];
    if (shellState.appState !== undefined && quickFilterDraft !== undefined) {
      shellState.appState = applyAppPatches(
        shellState.appState,
        deleteQuickFilterDraftPatch(sourceKey),
        setQuickFilterDraftPatch(targetKey, quickFilterDraft),
      );
    }
    if (shellState.openTerminalTabKeys.delete(sourceKey))
      shellState.openTerminalTabKeys.add(targetKey);
  }

  /** Releases every per-tab cache belonging to a workspace being switched away from. */
  function releaseWorkspaceTabState(outgoing: WorkspaceProjection): void {
    for (const paneId of outgoing.paneOrder) {
      for (const tabId of outgoing.panesById[paneId]?.tabOrder ?? []) {
        clearTabState(paneId, tabId);
      }
      shellState.editorByPane.get(paneId)?.controller.dispose();
      shellState.editorByPane.delete(paneId);
    }
  }

  /** Loads every pane's active tab, the currently active pane first (task 0084). */
  function loadPanesActiveFirst(loaded: WorkspaceProjection): void {
    void shellState.navigation.load(loaded.activePaneId);
    for (const paneId of loaded.paneOrder) {
      if (paneId !== loaded.activePaneId) void shellState.navigation.load(paneId);
    }
  }

  function workspaceErrorMessage(error: unknown, fallback: string): string {
    if (error instanceof Error) return error.message;
    if (typeof error === 'object' && error !== null) {
      const message = (error as { readonly message?: unknown }).message;
      if (typeof message === 'string' && message.length > 0) return message;
    }
    return fallback;
  }

  /** Flushes any pending layout edit and swaps in an already-fetched workspace projection. */

  /**
   * Switches the active workspace (task 0084): flushes any pending debounced
   * layout edit, releases the outgoing workspace's per-tab caches, restores
   * the target workspace's persisted layout, and loads its active pane's
   * tabs first. Never touches `operations` — running file operations must
   * survive a switch untouched.
   */
  async function switchWorkspace(workspaceId: WorkspaceId): Promise<void> {
    await shellState.workspaceController.switchWorkspace(workspaceId);
  }

  function refetchAffectedPanes(
    paneId?: PaneId,
    options?: {
      readonly background?: boolean;
      readonly settleMs?: number;
      readonly force?: boolean;
    },
  ): void {
    treeWiring.refreshDirectoryTree();
    basketWiring.scheduleBasketRefresh();
    if (shellState.workspace === undefined) return;
    const background = options?.background ?? true;
    for (const candidate of shellState.workspace.paneOrder) {
      const activeTab =
        shellState.workspace.panesById[candidate]?.tabsById[
          shellState.workspace.panesById[candidate]?.activeTabId ?? ''
        ];
      if (shellState.basketTabIds.has(shellState.workspace.panesById[candidate]?.activeTabId ?? ''))
        continue;
      if (activeTab?.location.uri.startsWith('search://')) continue;
      if (paneId !== undefined && candidate !== paneId) continue;
      // Background refreshes are used for opportunistic reloads (e.g. deltas/watch events),
      // while some callers (operation completion) request a foreground reload to guarantee
      // authoritative source/destination listings after mutating actions.
      if (options?.settleMs !== undefined) {
        scheduleSettledRefresh(candidate, options.settleMs, background, options.force === true);
        continue;
      }
      void shellState.navigation.load(candidate, background ? { background: true } : undefined);
    }
  }

  /**
   * Safety-net reload for a pane after a mutating operation. The backend already re-lists the
   * affected panes itself and pushes the result as a directory delta, so reloading immediately
   * made every operation cost two or three full relists of the same folder. This waits `settleMs`
   * and only reloads when no delta or reload reached the pane in the meantime, unless `force`.
   */
  function scheduleSettledRefresh(
    paneId: PaneId,
    settleMs: number,
    background: boolean,
    force: boolean,
  ): void {
    const startingRevision = shellState.directories.get(activeTabKey(paneId))?.revision;
    const pending = shellState.settledRefreshTimers.get(paneId);
    if (pending !== undefined) clearTimeout(pending);
    shellState.settledRefreshTimers.set(
      paneId,
      setTimeout(() => {
        shellState.settledRefreshTimers.delete(paneId);
        if (
          !force &&
          shellState.directories.get(activeTabKey(paneId))?.revision !== startingRevision
        )
          return;
        void shellState.navigation.load(paneId, background ? { background: true } : undefined);
      }, settleMs),
    );
  }

  function applyDelta(paneId: PaneId, delta: DirectoryDelta): void {
    const started = perfStart();
    try {
      applyDeltaUntimed(paneId, delta);
    } finally {
      perfEndAfterPaint('apply-delta-to-paint', started, { type: delta.type });
    }
  }

  function applyDeltaUntimed(paneId: PaneId, delta: DirectoryDelta): void {
    const key = activeTabKey(paneId);
    const current = shellState.directories.get(key);
    basketWiring.scheduleBasketRefresh();
    const revision = delta.type === 'reset' ? delta.snapshot.revision : delta.revision;
    if (current === undefined || current.revision === undefined) {
      refetchAffectedPanes(paneId);
      return;
    }
    if (revision <= current.revision) return;
    if (revision !== current.revision + 1 && delta.type !== 'reset') {
      refetchAffectedPanes(paneId);
      return;
    }
    if (delta.type !== 'reset' && current.hasMore) {
      refetchAffectedPanes(paneId);
      return;
    }
    if (delta.type === 'reset') {
      // Large directories are reset with only their first page. Applying that verbatim would
      // drop every later page the user already loaded and prune the cursor onto page one, so
      // re-fetch instead: background reloads re-hydrate all previously loaded pages.
      if (delta.snapshot.hasMore && delta.snapshot.entries.length < current.entries.length) {
        // The backend already holds the fresh sorted listing, so only the later pages are
        // re-fetched from its cache - a plain reload would make it re-list and re-sort the folder.
        void shellState.navigation.load(paneId, {
          background: true,
          firstSnapshot: delta.snapshot,
        });
        return;
      }
      shellState.navigation.applySnapshot(paneId, delta.snapshot);
      m.redraw();
      return;
    }
    const entries = [...current.entries];
    const byId = new Map(entries.map((entry) => [entry.id, entry]));
    if (delta.type === 'entriesRemoved') {
      if (!Array.isArray(delta.entryIds)) {
        refetchAffectedPanes(paneId);
        return;
      }
      for (const id of delta.entryIds) byId.delete(id);
    } else {
      if (!Array.isArray(delta.entries)) {
        refetchAffectedPanes(paneId);
        return;
      }
      for (const entry of delta.entries) byId.set(entry.id, entry);
    }
    const ordered = entries.flatMap((entry) => {
      const next = byId.get(entry.id);
      if (next === undefined) return [];
      byId.delete(entry.id);
      return [next];
    });
    const nextEntries = [...ordered, ...byId.values()];
    shellState.directories.set(key, { ...current, revision, entries: nextEntries });
    reconcileSelectionAfterEntryChange(
      paneId,
      shellState.workspace?.panesById[paneId]?.activeTabId,
      current.entries,
      nextEntries,
    );
    m.redraw();
  }

  function reconcileSelectionAfterEntryChange(
    paneId: PaneId,
    tabId: TabId | undefined,
    previousEntries: readonly EntrySummary[],
    nextEntries: readonly EntrySummary[],
  ): void {
    if (tabId === undefined) return;
    const key = tabKey(paneId, tabId);
    const pendingCreatedLocation = shellState.dialogs.getState().pendingCreatedLocation;
    const created =
      pendingCreatedLocation === undefined
        ? undefined
        : nextEntries.find((entry) => entry.location.uri === pendingCreatedLocation);
    if (created !== undefined) {
      shellState.pendingCursorRestorations.delete(key);
      shellState.selections.set(
        key,
        reduceSelection(emptySelection, { type: 'selectOnly', entryId: created.id }, [created.id]),
      );
      shellState.dialogs.setPendingCreatedLocation(undefined);
      return;
    }

    const selection = shellState.selections.get(key);
    if (selection === undefined) return;
    const pending = shellState.pendingCursorRestorations.get(key);
    if (
      pending !== undefined &&
      (Date.now() > pending.expiresAt || selection.cursorEntryId !== pending.fallbackCursorId)
    ) {
      shellState.pendingCursorRestorations.delete(key);
    }
    const nextIds = new Set(nextEntries.map((entry) => entry.id));
    const removedEntryIds = previousEntries
      .filter((entry) => !nextIds.has(entry.id))
      .map((entry) => entry.id);
    const removedCursor = previousEntries.find(
      (entry) => entry.id === selection.cursorEntryId && removedEntryIds.includes(entry.id),
    );
    const restoration = shellState.pendingCursorRestorations.get(key);
    const candidate =
      removedCursor === undefined && restoration === undefined
        ? undefined
        : nextEntries.find(
            (entry) =>
              entry.location.providerId ===
                (removedCursor?.location.providerId ?? restoration?.providerId) &&
              entry.location.uri === (removedCursor?.location.uri ?? restoration?.uri) &&
              entry.kind === (removedCursor?.kind ?? restoration?.kind),
          );
    if (removedEntryIds.length === 0 && candidate === undefined) return;

    const tab = shellState.workspace?.panesById[paneId]?.tabsById[tabId];
    const previousVisibleEntries = entriesFilteredFor(
      key,
      entriesSortedFor(
        key,
        previousEntries,
        effectiveSort(tab?.view.sort ?? []),
        tab?.view.foldersFirst ?? false,
        tab?.location.uri.startsWith('search://') === true,
      ),
      quickFilterQueryFor(key, tab),
    );
    const nextSelection =
      removedEntryIds.length === 0
        ? selection
        : reduceSelection(
            selection,
            { type: 'prune', removedEntryIds },
            previousVisibleEntries.map((entry) => entry.id),
          );
    if (candidate !== undefined) {
      shellState.pendingCursorRestorations.delete(key);
      const wasSelected =
        removedCursor !== undefined
          ? selection.selectedEntryIds.includes(removedCursor.id)
          : restoration?.wasSelected;
      const wasAnchor =
        removedCursor !== undefined
          ? selection.anchorEntryId === removedCursor.id
          : restoration?.wasAnchor;
      shellState.selections.set(key, {
        ...nextSelection,
        cursorEntryId: candidate.id,
        ...(wasSelected
          ? { selectedEntryIds: [...nextSelection.selectedEntryIds, candidate.id] }
          : {}),
        ...(wasAnchor ? { anchorEntryId: candidate.id } : {}),
      });
      return;
    }
    if (removedCursor !== undefined) {
      shellState.pendingCursorRestorations.set(key, {
        providerId: removedCursor.location.providerId,
        uri: removedCursor.location.uri,
        kind: removedCursor.kind,
        fallbackCursorId: nextSelection.cursorEntryId,
        wasSelected: selection.selectedEntryIds.includes(removedCursor.id),
        wasAnchor: selection.anchorEntryId === removedCursor.id,
        expiresAt: Date.now() + 3000,
      });
    }
    shellState.selections.set(key, nextSelection);
  }

  function activeDirectory(): { paneId: PaneId; location: Location } | undefined {
    const paneId = shellState.workspace?.activePaneId;
    const location =
      paneId === undefined ? undefined : shellState.directories.get(activeTabKey(paneId))?.location;
    return paneId === undefined || location === undefined ? undefined : { paneId, location };
  }

  /** A short display label for the tree sidebar's root row - the host segment of a remote
   * provider's URI (e.g. `sftp://my-server/` -> "my-server"), or "/" for a local root. */
  function treeRootName(location: Location): string {
    try {
      const host = new URL(location.uri).host;
      return host === '' ? '/' : host;
    } catch {
      return location.providerId;
    }
  }

  const backendEventContext: BackendEventContext = {
    getWorkspaceId: () => shellState.workspace?.id,
    getWorkspaceRevision: () => shellState.workspace?.revision,
    replaceWorkspace: applyRemoteWorkspaceSnapshot,
    refreshWorkspaceSummaries: () => shellState.workspaceController.refreshWorkspaceSummaries(),
    setWorkspaceSummaries: (summaries) => {
      shellState.workspaceSummaries = summaries;
    },
    setWorkspaceActionError: (message) => {
      shellState.workspaceActionError = message;
    },
    recoverActiveWorkspace: (summaries) =>
      shellState.workspaceController.recoverActiveWorkspace(summaries),
    listWorkspaces: () => shellState.client.listWorkspaces(),
    getWorkspace: (id) => shellState.client.getWorkspace(id),
    setPendingConflict: (conflict) => {
      shellState.pendingConflict = conflict;
    },
    getPendingOperationEvents: () => shellState.pendingOperationEvents,
    pushPendingOperationEvent: (event) => {
      shellState.pendingOperationEvents.push(event);
    },
    clearPendingOperationEvents: () => {
      const events = shellState.pendingOperationEvents;
      shellState.pendingOperationEvents = [];
      return events;
    },
    getOperationFrame: () => shellState.operationFrame,
    setOperationFrame: (frame) => {
      shellState.operationFrame = frame;
    },
    getOperations: () => shellState.operations,
    setOperations: (next) => {
      shellState.operations = next;
      if (summariseActiveOperations(next) === undefined && shellState.semanticJobs.size === 0) {
        shellState.activityOpen = false;
      }
    },
    onSemanticIngestionProgress: (progress) => {
      if (['complete', 'cancelled', 'skipped'].includes(progress.stage)) {
        shellState.semanticJobs.delete(progress.jobId);
        if (
          shellState.semanticJobs.size === 0 &&
          summariseActiveOperations(shellState.operations) === undefined
        ) {
          shellState.activityOpen = false;
        }
      } else {
        shellState.semanticJobs.set(progress.jobId, progress);
      }
    },
    getDismissedOperationIds: () => shellState.dismissedOperationIds,
    clearDismissedOperation: opsWiring.clearDismissedOperation,
    scheduleAutoDismiss: opsWiring.scheduleAutoDismiss,
    clearOperationSourceSelections: opsWiring.clearOperationSourceSelections,
    removeOperationSourcesFromSearchResults: opsWiring.removeOperationSourcesFromSearchResults,
    removeOperationSourcesFromDiskUsage: opsWiring.removeOperationSourcesFromDiskUsage,
    getActiveDirectoryRevision: (paneId) =>
      shellState.directories.get(activeTabKey(paneId))?.revision,
    applyDelta,
    refetchAffectedPanes,
    getPlugins: () => shellState.plugins,
    setPlugins: (next) => {
      shellState.plugins = next;
      for (const [paneId, panel] of shellState.pluginByPane) {
        if (!next.some((plugin) => plugin.id === panel.pluginId && plugin.enabled)) {
          shellState.tabController.performCloseTab(paneId, panel.tabId);
        }
      }
      void shellState.client
        .listActions()
        .then((actions) => {
          shellState.registeredActions = actions;
          m.redraw();
        })
        .catch(() => {
          toast({ html: t('action', 'unableToRun') });
        });
    },
    listPlugins: () => shellState.client.listPlugins(),
    getCurrentIconThemeSetting: () => shellState.currentSettings?.iconTheme,
    applyIconTheme: settingsWiring.applyIconTheme,
    getConnections: () => shellState.connections,
    setConnections: (next) => {
      shellState.connections = next;
    },
    getConnection: (id) => shellState.client.getConnection(id),
    getComparisonState: () => shellState.comparisonState,
    setComparisonState: (next) => {
      shellState.comparisonState = next;
    },
    getChecksumState: () => shellState.checksumState,
    setChecksumState: (next) => {
      shellState.checksumState = next;
    },
    getDuplicateState: () => shellState.duplicateState,
    setDuplicateState: (next) => {
      shellState.duplicateState = next;
    },
    markComparisonDifferences: (state) => {
      for (const paneId of [state.leftPaneId, state.rightPaneId]) {
        if (paneId === undefined) continue;
        const key = activeTabKey(paneId);
        const directory = shellState.directories.get(key);
        if (directory === undefined) continue;
        const matchingIds = differingEntryIds(state, paneId, directory.entries);
        if (matchingIds.length === 0) continue;
        const orderedEntryIds = directory.entries.map((entry) => entry.id);
        shellState.selections.set(
          key,
          reduceSelection(
            shellState.selections.get(key) ?? emptySelection,
            { type: 'restore', entryIds: matchingIds },
            orderedEntryIds,
          ),
        );
      }
    },
    getFindFilesSearchId: () => shellState.findFilesSearchId,
    getFindFilesTargetPane: (searchId) => shellState.findFilesTargetPaneBySearchId.get(searchId),
    clearFindFilesTargetPane: (searchId) => {
      shellState.findFilesTargetPaneBySearchId.delete(searchId);
    },
    hasPendingFindFilesStart: () => shellState.pendingFindFilesStarts > 0,
    deferSearchResultBatch: (event) => {
      if (event.payload.type !== 'search.resultsBatch') return;
      const pending = shellState.deferredSearchResultBatches.get(event.payload.searchId) ?? [];
      pending.push(event);
      shellState.deferredSearchResultBatches.set(event.payload.searchId, pending);
    },
    setSearchExecutionMode: (uri, executionMode) => {
      const searchId = uri.slice('search://local/'.length);
      shellState.findFilesExecutionModesBySearchId.set(searchId, executionMode);
      const presentation = shellState.findFilesPresentationsByLocationUri.get(uri);
      if (presentation !== undefined) {
        shellState.findFilesPresentationsByLocationUri.set(uri, { ...presentation, executionMode });
      }
    },
    cacheContentMatches: (uri, matches) => {
      if (shellState.appState !== undefined)
        shellState.appState = applyAppPatches(
          shellState.appState,
          cacheContentMatchesPatch(uri, matches),
        );
    },
    findPanesWithUri: (uri) =>
      shellState.workspace === undefined
        ? []
        : (
            Object.entries(shellState.workspace.panesById) as Array<
              [PaneId, WorkspaceProjection['panesById'][PaneId]]
            >
          )
            .filter(([, pane]) => pane.tabsById[pane.activeTabId]?.location.uri === uri)
            .map(([paneId]) => paneId),
    revealSearchResults: async (searchId) => {
      const searchUri = `search://local/${searchId}`;
      const paneId = shellState.findFilesTargetPaneBySearchId.get(searchId);
      if (paneId === undefined) return [];
      await shellState.navigation.navigate(paneId, { providerId: 'search', uri: searchUri });
      shellState.focusPane?.(paneId);
      m.redraw();
      return [paneId];
    },
    loadPane: (paneId, options) => shellState.navigation.load(paneId, options),
    reportSearchCompletion: (paneId, searchId) => {
      const searchUri = `search://local/${searchId}`;
      const active = activeDirectory();
      // Only react while the pane is still showing this exact search: the user may have already
      // navigated elsewhere, or started a newer search, by the time results finish streaming in.
      if (active === undefined || active.paneId !== paneId || active.location.uri !== searchUri) {
        return;
      }
      const entries = shellState.directories.get(activeTabKey(paneId))?.entries ?? [];
      if (entries.length > 0) return;
      if (shellState.findFilesPresentationsByLocationUri.get(searchUri)?.conceptSearch) return;
      toast({ html: t('search', 'noResultsToast') });
      const root = shellState.findFilesRootsByLocationUri.get(searchUri);
      if (root !== undefined) void shellState.navigation.navigate(paneId, root);
    },
    reportSearchWithoutResults: (searchId) => {
      if (
        shellState.findFilesPresentationsByLocationUri.get(`search://local/${searchId}`)
          ?.conceptSearch
      ) {
        return;
      }
      toast({ html: t('search', 'noResultsToast') });
    },
    applyDiskUsageProgress: opsWiring.applyDiskUsageProgress,
    applyDiskUsageFinalizing: opsWiring.applyDiskUsageFinalizing,
    applyDiskUsageFailure: opsWiring.applyDiskUsageFailure,
    redraw: () => m.redraw(),
  };
  const handleBackendEvent = createBackendEventHandler(backendEventContext);

  /** `dispose()` of the `buildControllers` registry set up in `oninit` (controller-registry.ts). */
  let disposeShellControllers: (() => void) | undefined;
  let volumePollTimer: number | undefined;
  let cacheStatsTimer: number | undefined;
  const CACHE_STATS_INTERVAL_MS = 30_000;
  const VOLUME_POLL_INTERVAL_MS = 10_000;

  const workspaceControllerContext: WorkspaceControllerContext = {
    getWorkspace: () => shellState.workspace,
    onInitialWorkspaceReady: (freshInstall) => {
      if (!freshInstall) return;
      try {
        const key = 'procyon.walkthroughInvitationShown';
        if (localStorage.getItem(key) !== null) return;
        localStorage.setItem(key, '1');
        shellState.walkthroughInvitationVisible = true;
      } catch (error: unknown) {
        console.warn('Could not persist the walkthrough invitation; it will not be shown.', error);
      }
    },
    setWorkspace: (ws) => {
      shellState.workspace = ws;
    },
    getWorkspaceError: () => shellState.workspaceError,
    setWorkspaceError: (msg) => {
      shellState.workspaceError = msg;
    },
    getWorkspaceSummaries: () => shellState.workspaceSummaries,
    setWorkspaceSummaries: (summaries) => {
      shellState.workspaceSummaries = summaries;
    },
    getWorkspaceActionError: () => shellState.workspaceActionError,
    setWorkspaceActionError: (msg) => {
      shellState.workspaceActionError = msg;
    },
    getWorkspaceRequest: () => shellState.workspaceRequest,
    setWorkspaceRequest: (ac) => {
      shellState.workspaceRequest = ac;
    },
    getPlatform: () => shellState.platform,
    setPlatform: (p) => {
      shellState.platform = p;
    },
    getNativeDragOutSupported: () => shellState.nativeDragOutSupported,
    setNativeDragOutSupported: (v) => {
      shellState.nativeDragOutSupported = v;
    },
    getUnsubscribeNativeFileDrops: () => shellState.unsubscribeNativeFileDrops,
    setUnsubscribeNativeFileDrops: (fn) => {
      shellState.unsubscribeNativeFileDrops = fn;
    },
    subscribeNativeFileDrops: (callback) => shellState.client.subscribeNativeFileDrops(callback),
    getDraggedLocations: () => shellState.draggedLocations,
    getNativeDragSourceInternal: () => shellState.nativeDragSourceInternal,
    setNativeDragSourceInternal: (v) => {
      shellState.nativeDragSourceInternal = v;
    },
    setOpenTerminalSupported: (v) => {
      shellState.openTerminalSupported = v;
    },
    setPlatformContextMenuSupported: (v) => {
      shellState.platformContextMenuSupported = v;
    },
    setNativeIconLoader: (loader) => {
      shellState.nativeIconLoaderSource = loader;
      shellState.nativeIconLoader =
        shellState.currentSettings?.iconTheme === 'native' ? loader : undefined;
    },
    setThumbnailLoader: (loader) => {
      shellState.thumbnailLoader = loader;
    },
    setFinderTagsLoader: (loader) => {
      shellState.finderTagsLoader = loader;
    },
    getRuntimeCapabilities: () => shellState.runtimeCapabilities,
    setRuntimeCapabilities: (capabilities) => {
      shellState.runtimeCapabilities = capabilities;
    },
    getSystemLocations: () => shellState.systemLocations,
    setSystemLocations: (locs) => {
      shellState.systemLocations = locs;
    },
    setSystemLocationsError: (msg) => {
      shellState.systemLocationsError = msg;
    },
    getVolumes: () => shellState.volumes,
    setVolumes: (vols) => {
      shellState.volumes = vols;
    },
    setVolumesError: (msg) => {
      shellState.volumesError = msg;
    },
    setHomeDirectory: (path) => {
      shellState.homeDirectory = path;
    },
    getConnections: () => shellState.connections,
    setConnections: (conns) => {
      shellState.connections = conns;
    },
    setDraggedLocations: (locs) => {
      shellState.draggedLocations = locs;
    },
    getNativeDropInProgress: () => shellState.nativeDropInProgress,
    setNativeDropInProgress: (v) => {
      shellState.nativeDropInProgress = v;
    },
    setClipboardMessage: (msg) => {
      shellState.clipboardMessage = msg;
    },
    getNavigation: () => shellState.navigation,
    getFlushPendingLayoutUpdate: () => shellState.flushPendingLayoutUpdate,
    redraw: () => m.redraw(),
    releaseWorkspaceTabState: (outgoing) => releaseWorkspaceTabState(outgoing),
    loadPanesActiveFirst: (ws) => loadPanesActiveFirst(ws),
    syncWorkspaceViewSettings: () => {
      if (shellState.currentSettings !== undefined) {
        void shellState.settingsController.applyShowHiddenFilesToAllTabs(
          shellState.client,
          shellState.currentSettings.showHiddenFiles,
        );
      }
    },
  };

  const tabControllerContext: TabControllerContext = {
    getWorkspace: () => shellState.workspace,
    setWorkspace: (ws) => {
      shellState.workspace = ws;
    },
    getAppState: () => shellState.appState,
    setAppState: (state) => {
      shellState.appState = state;
    },
    getNavigation: () => shellState.navigation,
    redraw: () => m.redraw(),
    applyCurrentShowHiddenSetting: (client, workspaceId, paneId, tabId, rev) =>
      shellState.settingsController.applyCurrentShowHiddenSetting(
        client,
        workspaceId,
        paneId,
        tabId,
        rev,
      ),
    clearTabState,
    getCloseTabConfirmation: () => shellState.closeTabConfirmation,
    setCloseTabConfirmation: (conf) => {
      shellState.closeTabConfirmation = conf;
    },
    hasCachedSnapshot: (paneId, tabId) =>
      shellState.directories.get(tabKey(paneId, tabId))?.state.type === 'loaded',
  };

  const settingsControllerContext: SettingsControllerContext = {
    setTheme: (t) => {
      shellState.theme = t;
    },
    setLoadedEntryFormatSettings: (s) => {
      shellState.loadedEntryFormatSettings = s;
    },
    getSettingsDialogOpen: () => shellState.settingsDialogOpen,
    setSettingsDialogOpen: (open) => {
      shellState.settingsDialogOpen = open;
    },
    getSettingsDisclosureElement: () => shellState.settingsDisclosureElement,
    getCurrentSettings: () => shellState.currentSettings,
    setCurrentSettings: (s) => {
      shellState.currentSettings = s;
    },
    getPlugins: () => shellState.plugins,
    getInstalledIconThemeId: () => shellState.installedIconThemeId,
    setInstalledIconThemeId: (id) => {
      shellState.installedIconThemeId = id;
    },
    setNativeIconLoaderEnabled: (enabled) => {
      shellState.nativeIconLoader = enabled ? shellState.nativeIconLoaderSource : undefined;
    },
    getRuntimeKind: () => shellState.runtimeKind,
    getWorkspace: () => shellState.workspace,
    setWorkspace: (ws) => {
      shellState.workspace = ws;
    },
    getDirectories: () => shellState.directories,
    getNavigation: () => shellState.navigation,
    getClient: () => shellState.client,
    redraw: () => m.redraw(),
  };

  const globalKeydownHandlerContext: GlobalKeydownContext = {
    getCommandPaletteOpen: () => shellState.commandPaletteOpen,
    getPlatform: () => shellState.platform,
    getKeybindingRuntime: () => shellState.keybindingRuntime,
    getCurrentSettings: () => shellState.currentSettings,
    getWorkspace: () => shellState.workspace,
    getSelections: () => shellState.selections,
    getDirectories: () => shellState.directories,
    getRegisteredActions: actionsWiring.keybindingActions,
    getPlugins: () => shellState.plugins,
    collectIntoBasketIfVisible: () => {
      if (!basketWiring.basketVisible()) return false;
      if (
        shellState.workspace !== undefined &&
        shellState.basketTabIds.has(
          shellState.workspace.panesById[shellState.workspace.activePaneId]?.activeTabId ?? '',
        )
      ) {
        return false;
      }
      basketWiring.collectSelection();
      return true;
    },
    removeFromBasketIfActive: () => {
      if (shellState.workspace === undefined) return false;
      return basketWiring.removeSelectedBasketItems(shellState.workspace.activePaneId);
    },
    prepareFileOperationEntries: basketWiring.prepareFileOperationEntries,
    clipboard: miscWiring.clipboard,
    getFindFilesOpen: () => shellState.findFilesOpen,
    getViewer: (paneId) => {
      const tabId = shellState.workspace?.panesById[paneId]?.activeTabId;
      return tabId === undefined ? undefined : shellState.viewerByTab.get(tabKey(paneId, tabId));
    },
    getArchiveCreateRequest: () => shellState.dialogs.getState().archiveCreateRequest,
    getCreateDirectoryOpen: () => shellState.dialogs.getState().createDirectoryOpen,
    getCreateFileOpen: () => shellState.dialogs.getState().createFileOpen,
    getCreateLinkOpen: () => shellState.dialogs.getState().createLinkRequest !== undefined,
    getAppState: () => shellState.appState,
    getLastQuickFilterQuery: (paneId) =>
      shellState.lastQuickFilterQueryByTabKey.get(activeTabKey(paneId)),
    getShortcutsHelpOpen: () => shellState.shortcutsHelpOpen,
    setCommandPaletteOpen: (open) => {
      shellState.commandPaletteOpen = open;
    },
    setClipboardMessage: (msg) => {
      shellState.clipboardMessage = msg;
    },
    setArchiveCreateRequest: (req) => {
      if (req !== undefined) shellState.dialogs.openArchiveCreate(req);
    },
    setCreateDirectoryOpen: (open) => {
      if (open) shellState.dialogs.openCreateDirectory();
      else shellState.dialogs.cancelCreateDirectory();
    },
    setCreateFileOpen: (open) => {
      if (open) shellState.dialogs.openCreateFile();
      else shellState.dialogs.cancelCreateFile();
    },
    openCreateLink: (target, destination) =>
      shellState.dialogs.openCreateLink({ target, destination }),
    setAppState: (state) => {
      shellState.appState = state;
    },
    setQuickFilterOpen: (key, open) => {
      shellState.quickFilterOpen.set(key, open);
    },
    setActiveTabQuickFilter: (paneId, query) => {
      const liveWorkspace = shellState.workspace;
      const pane = liveWorkspace?.panesById[paneId];
      const tab = pane === undefined ? undefined : pane.tabsById[pane.activeTabId];
      if (liveWorkspace === undefined || tab === undefined) return;
      const key = activeTabKey(paneId);
      const previous = tab.view.quickFilter?.query ?? '';
      if (query === undefined) {
        if (previous.length > 0) shellState.lastQuickFilterQueryByTabKey.set(key, previous);
        shellState.quickFilterOpen.set(key, false);
      } else {
        shellState.quickFilterOpen.set(key, true);
      }
      if (shellState.appState !== undefined)
        shellState.appState = applyAppPatches(
          shellState.appState,
          deleteQuickFilterDraftPatch(key),
        );
      void dispatchWorkspaceCommand(
        shellState.client,
        {
          type: 'updateView',
          workspaceId: liveWorkspace.id,
          paneId,
          tabId: tab.id,
          patch: {
            quickFilter:
              query === undefined ? { type: 'clear' } : { type: 'set', filter: { query } },
          },
          expectedRevision: liveWorkspace.revision,
        },
        (next) => {
          shellState.workspace = next;
        },
      ).catch(() => undefined);
    },
    setConnectionsManagerOpen: (open) => {
      shellState.connectionsManagerOpen = open;
    },
    setShortcutsHelpOpen: (open) => {
      shellState.shortcutsHelpOpen = open;
    },
    getTabController: () => shellState.tabController,
    getOpsController: () => shellState.opsController,
    getNavigation: () => shellState.navigation,
    activeDirectory,
    activeTabKey,
    actionsWithFavourites: actionsWiring.actionsWithFavourites,
    openFindFiles: () => shellState.findFilesController.openFindFiles(),
    toggleBranchView: () => shellState.findFilesController.toggleBranchView(),
    toggleSyncBrowsing: () => shellState.syncedBrowsing?.toggle(),
    replaceClipboard: miscWiring.replaceClipboard,
    resolvePasteClipboard: miscWiring.resolvePasteClipboard,
    selectedLocations: miscWiring.selectedLocations,
    invokeActionById: (actionId, parameters, context) =>
      shellState.actionCommandController.invokeActionById(actionId, parameters, context),
    openViewer: (paneId, entry, initialSearch, openMetadata) =>
      previewWiring.openViewer(shellState.client, paneId, entry, initialSearch, openMetadata),
    openEditor: (paneId, entry) => previewWiring.openEditor(shellState.client, paneId, entry),
    calculateFolderSize: (paneId, entry) => calculateFolderSize(shellState.client, paneId, entry),
    uninstallApplication: (paneId, entry) => uninstallApplication(shellState.client, paneId, entry),
    actionContext: () => shellState.actionCommandController.actionContext(),
    commandAvailabilityContext: (selectedEntries, paneId) =>
      shellState.actionCommandController.commandAvailabilityContext(selectedEntries, paneId),
    contentSearchInitialQuery,
    pdfImagePageForEntry,
    refetchAffectedPanes,
    platformActionParameters: (actionId, selectedEntries, directoryLocation) =>
      shellState.actionCommandController.platformActionParameters(
        actionId,
        selectedEntries,
        directoryLocation,
      ),
    activatePane: (paneId) => activatePane(shellState.client, paneId),
    focusPane: (paneId) => {
      if (shellState.focusPane !== undefined) shellState.focusPane(paneId);
      else void activatePane(shellState.client, paneId);
    },
    focusViewer: miscWiring.focusViewer,
    focusViewerSearch: miscWiring.focusViewerSearch,
    scrollViewer: miscWiring.scrollViewer,
    toggleTerminal: () => {
      if (shellState.runtimeKind !== 'tauri') return;
      const key = treeWiring.activeTerminalTabKey();
      if (key === undefined) return;
      if (shellState.openTerminalTabKeys.has(key)) {
        shellState.openTerminalTabKeys.delete(key);
      } else {
        shellState.openTerminalTabKeys.add(key);
        focusOpenedTerminal(
          () => m.redraw.sync(),
          () => shellState.focusTerminal?.() ?? false,
        );
      }
    },
    toggleDirectoryTree: treeWiring.toggleDirectoryTree,
    toggleOperationCentre: opsWiring.toggleOperationCentre,
    redraw: () => m.redraw(),
    setSort: (paneId, sort) => {
      const liveWorkspace = shellState.workspace;
      const pane = liveWorkspace?.panesById[paneId];
      const tab = pane === undefined ? undefined : pane.tabsById[pane.activeTabId];
      if (liveWorkspace === undefined || tab === undefined) return;
      void dispatchWorkspaceCommand(
        shellState.client,
        {
          type: 'updateView',
          workspaceId: liveWorkspace.id,
          paneId,
          tabId: tab.id,
          patch: { sort: [...sort] },
          expectedRevision: liveWorkspace.revision,
        },
        (next) => {
          shellState.workspace = next;
        },
      )
        .then(() => {
          if (shellState.workspace?.panesById[paneId]?.activeTabId === tab.id) {
            void shellState.navigation.load(paneId, { background: true });
          }
        })
        .catch(() => undefined);
    },
    swapPaneTabSets: (paneAId, paneBId) => {
      const liveWorkspace = shellState.workspace;
      if (liveWorkspace === undefined) return;
      const paneA = liveWorkspace.panesById[paneAId];
      const paneB = liveWorkspace.panesById[paneBId];
      if (paneA === undefined || paneB === undefined) return;
      if (shellState.imageMapTab?.paneId === paneAId)
        shellState.imageMapTab = { ...shellState.imageMapTab, paneId: paneBId };
      else if (shellState.imageMapTab?.paneId === paneBId)
        shellState.imageMapTab = { ...shellState.imageMapTab, paneId: paneAId };
      const movedDiskUsage = new Map<string, DiskUsageTabEntry>();
      for (const tabId of paneA.tabOrder) {
        const entry = shellState.diskUsageByTab.get(tabKey(paneAId, tabId));
        if (entry !== undefined) {
          shellState.diskUsageByTab.delete(tabKey(paneAId, tabId));
          entry.key = tabKey(paneBId, tabId);
          entry.paneId = paneBId;
          movedDiskUsage.set(entry.key, entry);
        }
      }
      for (const tabId of paneB.tabOrder) {
        const entry = shellState.diskUsageByTab.get(tabKey(paneBId, tabId));
        if (entry !== undefined) {
          shellState.diskUsageByTab.delete(tabKey(paneBId, tabId));
          entry.key = tabKey(paneAId, tabId);
          entry.paneId = paneAId;
          movedDiskUsage.set(entry.key, entry);
        }
      }
      for (const [key, entry] of movedDiskUsage) shellState.diskUsageByTab.set(key, entry);
      const movedKnowledgeSearch = new Map<string, KnowledgeSearchTabEntry>();
      for (const tabId of paneA.tabOrder) {
        const entry = shellState.knowledgeSearchByTab.get(tabKey(paneAId, tabId));
        if (entry !== undefined) {
          shellState.knowledgeSearchByTab.delete(entry.key);
          entry.key = tabKey(paneBId, tabId);
          entry.paneId = paneBId;
          movedKnowledgeSearch.set(entry.key, entry);
        }
      }
      for (const tabId of paneB.tabOrder) {
        const entry = shellState.knowledgeSearchByTab.get(tabKey(paneBId, tabId));
        if (entry !== undefined) {
          shellState.knowledgeSearchByTab.delete(entry.key);
          entry.key = tabKey(paneAId, tabId);
          entry.paneId = paneAId;
          movedKnowledgeSearch.set(entry.key, entry);
        }
      }
      for (const [key, entry] of movedKnowledgeSearch)
        shellState.knowledgeSearchByTab.set(key, entry);
      const movedThemes = new Map<string, ThemesTabEntry>();
      for (const [paneId, otherPaneId, tabs] of [
        [paneAId, paneBId, paneA.tabOrder],
        [paneBId, paneAId, paneB.tabOrder],
      ] as const) {
        for (const tabId of tabs) {
          const entry = shellState.themesByTab.get(tabKey(paneId, tabId));
          if (entry === undefined) continue;
          shellState.themesByTab.delete(entry.key);
          entry.key = tabKey(otherPaneId, tabId);
          entry.paneId = otherPaneId;
          movedThemes.set(entry.key, entry);
        }
      }
      for (const [key, entry] of movedThemes) shellState.themesByTab.set(key, entry);
      // No backend command swaps a whole tab set atomically (task 0128 Agent Notes) - this
      // mutates the local projection directly, the same optimistic-update pattern
      // `activateTab` uses, rather than round-tripping through `dispatchWorkspaceCommand`.
      shellState.workspace = {
        ...liveWorkspace,
        panesById: {
          ...liveWorkspace.panesById,
          [paneAId]: {
            ...paneA,
            tabOrder: paneB.tabOrder,
            tabsById: paneB.tabsById,
            activeTabId: paneB.activeTabId,
          },
          [paneBId]: {
            ...paneB,
            tabOrder: paneA.tabOrder,
            tabsById: paneA.tabsById,
            activeTabId: paneA.activeTabId,
          },
        },
      };
      void shellState.navigation.load(paneAId);
      void shellState.navigation.load(paneBId);
    },
    requestLocationEdit: (paneId) => requestLocationEdit(paneId),
    openMultiRenameForActivePane: () => {
      const active = activeDirectory();
      if (active === undefined) return;
      const key = activeTabKey(active.paneId);
      const directory = shellState.directories.get(key);
      if (directory === undefined) return;
      const selection = shellState.selections.get(key);
      const selected = getSelectedEntries(selection, directory.entries).filter(
        (entry) => !isParentEntry(entry.id),
      );
      // Total Commander's Multi Rename Tool defaults to every entry in the directory when
      // nothing is selected, rather than requiring a selection first.
      const entriesToRename =
        selected.length > 0
          ? selected
          : directory.entries.filter((entry) => !isParentEntry(entry.id));
      const firstEntry = entriesToRename[0];
      if (firstEntry === undefined) return;
      const firstParent = parentLocation(firstEntry.location);
      const singleParent = entriesToRename.every((entry) => {
        const parent = parentLocation(entry.location);
        return parent.providerId === firstParent.providerId && parent.uri === firstParent.uri;
      });
      const selectedIds = new Set(entriesToRename.map((entry) => entry.id));
      shellState.dialogs.openMultiRename(
        entriesToRename,
        singleParent ? firstParent : undefined,
        new Set(
          singleParent
            ? directory.entries
                .filter((entry) => !selectedIds.has(entry.id))
                .map((entry) => entry.name)
            : [],
        ),
      );
      m.redraw();
    },
    openPropertiesForActivePane: () => {
      const active = activeDirectory();
      if (active === undefined) return;
      const key = activeTabKey(active.paneId);
      const directory = shellState.directories.get(key);
      if (directory === undefined) return;
      const selection = shellState.selections.get(key);
      const entries = getSelectedEntriesOrCursor(selection, directory.entries).filter(
        (entry) => !isParentEntry(entry.id),
      );
      if (entries.length === 0) return;
      shellState.dialogs.openProperties(entries);
      m.redraw();
    },
    quitApplication: () => {
      if (shellState.keybindingRuntime !== 'desktop') return;
      void shellState.client.quit?.();
    },
    startComparison: () => shellState.comparisonController.startComparison('sizeAndTimestamp'),
    calculateChecksums: () => shellState.checksumController.calculateChecksums(['sha256']),
    findDuplicates: () => shellState.checksumController.findDuplicates(),
    openDiskUsage: opsWiring.openDiskUsage,
    invokeClientAction: dialogWiring.invokeClientAction,
    openSemanticAssistant: knowledgeWiring.openSemanticAssistant,
    openKnowledgeSearch: knowledgeWiring.openKnowledgeSearch,
    openSettingsDialog: settingsWiring.openSettingsDialog,
  };

  const findFilesControllerContext: FindFilesControllerContext = {
    getFindFilesOpen: () => shellState.findFilesOpen,
    setFindFilesOpen: (open) => {
      shellState.findFilesOpen = open;
    },
    getFindFilesRoot: () => shellState.findFilesRoot,
    setFindFilesRoot: (root) => {
      shellState.findFilesRoot = root;
    },
    getFindFilesSearchId: () => shellState.findFilesSearchId,
    setFindFilesSearchId: (searchId) => {
      shellState.findFilesSearchId = searchId;
      if (searchId !== undefined) {
        for (const event of shellState.deferredSearchResultBatches.get(searchId) ?? []) {
          handleBackendEvent(event);
        }
        shellState.deferredSearchResultBatches.delete(searchId);
      }
    },
    setFindFilesTargetPane: (searchId, paneId) => {
      shellState.findFilesTargetPaneBySearchId.set(searchId, paneId);
      for (const event of shellState.deferredSearchResultBatches.get(searchId) ?? []) {
        handleBackendEvent(event);
      }
      shellState.deferredSearchResultBatches.delete(searchId);
    },
    clearFindFilesTargetPane: (searchId) => {
      shellState.findFilesTargetPaneBySearchId.delete(searchId);
    },
    setFindFilesSearchStartPending: (pending) => {
      shellState.pendingFindFilesStarts = Math.max(
        0,
        shellState.pendingFindFilesStarts + (pending ? 1 : -1),
      );
      if (shellState.pendingFindFilesStarts === 0) shellState.deferredSearchResultBatches.clear();
      m.redraw();
    },
    getFindFilesError: () => shellState.findFilesError,
    setFindFilesError: (error) => {
      shellState.findFilesError = error;
    },
    getFindFilesGeneration: () => shellState.findFilesGeneration,
    setFindFilesGeneration: (generation) => {
      shellState.findFilesGeneration = generation;
    },
    getFindFilesRootsByLocationUri: () => shellState.findFilesRootsByLocationUri,
    getFindFilesPresentationsByLocationUri: () => shellState.findFilesPresentationsByLocationUri,
    getFindFilesParamsByLocationUri: () => shellState.findFilesParamsByLocationUri,
    getFindFilesQueriesByLocationUri: () => shellState.findFilesQueriesByLocationUri,
    getSearchExecutionMode: (searchId) =>
      shellState.findFilesExecutionModesBySearchId.get(searchId),
    getActiveDirectory: () => activeDirectory(),
    getWorkspace: () => shellState.workspace,
    getClient: () => shellState.client,
    getPaneLocationUri: (paneId) => {
      const pane = shellState.workspace?.panesById[paneId];
      return pane?.tabsById[pane.activeTabId]?.location.uri;
    },
    openTabAt: (paneId, location, historyOrigin) =>
      shellState.tabController.openTabAt(paneId, location, historyOrigin),
    navigateTo: (paneId, location) => void shellState.navigation.navigate(paneId, location),
    reportLimitations: (message) => toast({ html: t('search', 'filterLimitations', { message }) }),
    redraw: () => m.redraw(),
  };

  const comparisonControllerContext: ComparisonControllerContext = {
    getState: () => shellState.comparisonState,
    setState: (next) => {
      shellState.comparisonState = next;
    },
    getWorkspace: () => shellState.workspace,
    getClient: () => shellState.client,
    redraw: () => m.redraw(),
  };

  const syncProfileControllerContext: SyncProfileControllerContext = {
    getClient: () => shellState.client,
    getSettings: () => shellState.currentSettings,
    updateSettings: (update) => updateLocationSettings(shellState.client, update),
    getRunTarget: () =>
      shellState.workspace === undefined
        ? undefined
        : { workspaceId: shellState.workspace.id, paneId: shellState.workspace.activePaneId },
    notify: (message) => toast({ html: message }),
    redraw: () => m.redraw(),
    newId: () => crypto.randomUUID(),
    now: () => new Date(),
    sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)),
  };

  /** Folders of the first two panes, offered as roots when editing a synchronization profile. */
  function syncProfilePaneLocations(): { left?: Location; right?: Location } {
    const current = shellState.workspace;
    if (current === undefined) return {};
    const locationOf = (index: number): Location | undefined => {
      const pane = current.panesById[current.paneOrder[index] ?? ''];
      return pane?.tabsById[pane.activeTabId]?.location;
    };
    const left = locationOf(0);
    const right = locationOf(1);
    return {
      ...(left === undefined ? {} : { left }),
      ...(right === undefined ? {} : { right }),
    };
  }

  const checksumControllerContext: ChecksumControllerContext = {
    getChecksumState: () => shellState.checksumState,
    setChecksumState: (next) => {
      shellState.checksumState = next;
    },
    getDuplicateState: () => shellState.duplicateState,
    setDuplicateState: (next) => {
      shellState.duplicateState = next;
    },
    getWorkspace: () => shellState.workspace,
    getClient: () => shellState.client,
    getSelectedEntries: () => {
      const active = activeDirectory();
      if (active === undefined) return [];
      const key = activeTabKey(active.paneId);
      return getSelectedEntriesOrCursor(
        shellState.selections.get(key),
        shellState.directories.get(key)?.entries ?? [],
      );
    },
    getActiveLocation: () => activeDirectory()?.location,
    // Reuses the same operation call `core.delete` makes, so duplicate
    // deletion inherits its confirmation, conflict handling and audit trail
    // instead of introducing a second delete path (spec §35, task 0077).
    requestDelete: (locations) => {
      if (locations.length === 0) return;
      void shellState.opsController.delete(
        [...locations],
        shellState.currentSettings?.confirmPermanentDelete === false,
        false,
      );
    },
    redraw: () => m.redraw(),
  };

  const requestLocationEdit = (paneId: PaneId): void => {
    shellState.locationEditRequests.set(
      paneId,
      (shellState.locationEditRequests.get(paneId) ?? 0) + 1,
    );
    m.redraw();
  };
  const actionCommandControllerContext: ActionCommandControllerContext = {
    getCommandPaletteOpen: () => shellState.commandPaletteOpen,
    setCommandPaletteOpen: (open) => {
      shellState.commandPaletteOpen = open;
    },
    getContextMenu: () => shellState.contextMenu,
    setContextMenu: (menu) => {
      shellState.contextMenu = menu;
    },
    getCommandPaletteRecency: () => shellState.commandPaletteRecency,
    getActiveDirectory: () => activeDirectory(),
    getActiveTabKey: (paneId) => activeTabKey(paneId),
    getSelections: () => shellState.selections,
    dispatchSelectionAction: (paneId, action) => {
      const dispatch = shellState.selectionDispatchers.get(paneId);
      if (dispatch === undefined) return false;
      dispatch(action);
      return true;
    },
    getDirectories: () => shellState.directories,
    getCurrentSettings: () => shellState.currentSettings,
    getClient: () => shellState.client,
    openPluginPane: previewWiring.openPluginPane,
    getRegisteredActions: () => [
      ...shellState.registeredActions,
      ...actionsWiring.mediaQueueActions(),
    ],
    getPlugins: () => shellState.plugins,
    getWorkspace: () => shellState.workspace,
    getNavigation: () => shellState.navigation,
    getOpsController: () => shellState.opsController,
    getGetSelectedEntries: () => getSelectedEntriesOrCursor,
    prepareFileOperationEntries: basketWiring.prepareFileOperationEntries,
    getClipboard: () => miscWiring.clipboard(),
    replaceClipboard: (next) => miscWiring.replaceClipboard(next),
    resolvePasteClipboard: miscWiring.resolvePasteClipboard,
    toast: (options) => toast(options),
    getOpenTerminalSupported: () => shellState.openTerminalSupported,
    openCreateDirectory: (location) => shellState.dialogs.openCreateDirectory(location),
    openFinderTagsDialog: (request) => shellState.dialogs.openFinderTagsDialog(request),
    openSpotlightCommentDialog: (request) => shellState.dialogs.openSpotlightCommentDialog(request),
    setArchiveCreateRequest: (request) => shellState.dialogs.openArchiveCreate(request),
    calculateChecksums: () => shellState.checksumController.calculateChecksums(['sha256']),
    findDuplicates: () => shellState.checksumController.findDuplicates(),
    openDiskUsage: opsWiring.openDiskUsage,
    invokeClientAction: dialogWiring.invokeClientAction,
    openPropertiesForActivePane: () => globalKeydownHandlerContext.openPropertiesForActivePane(),
    openMultiRenameForActivePane: () => globalKeydownHandlerContext.openMultiRenameForActivePane(),
    toggleBranchView: () => shellState.findFilesController.toggleBranchView(),
    toggleSyncBrowsing: () => shellState.syncedBrowsing?.toggle(),
    openDocumentSummary: (_paneId, entry) => {
      if (!shellState.semanticAssistantAvailable) return;
      if (shellState.workspace === undefined) return;
      shellState.dialogs.openDocumentSummaryDialog({ workspaceId: shellState.workspace.id, entry });
      m.redraw();
    },
    openSemanticAssistant: knowledgeWiring.openSemanticAssistant,
    openKnowledgeSearch: knowledgeWiring.openKnowledgeSearch,
    openThemes: knowledgeWiring.openThemes,
    uninstallApplication: (paneId, entry) =>
      globalKeydownHandlerContext.uninstallApplication(paneId, entry),
    requestRename: (paneId, entryIds) => {
      const revision = (shellState.renameRequests.get(paneId)?.revision ?? 0) + 1;
      shellState.renameRequests.set(paneId, { revision, entryIds });
      m.redraw();
    },
    requestLocationEdit,
    openCreateLink: (target, destination) =>
      shellState.dialogs.openCreateLink({ target, destination }),
    toggleDirectoryTree: treeWiring.toggleDirectoryTree,
    toggleOperationCentre: opsWiring.toggleOperationCentre,
    redraw: () => m.redraw(),
  };

  const paneContentBuilderContext: PaneContentContext = {
    getWorkspace: () => shellState.workspace,
    getCurrentSettings: () => shellState.currentSettings,
    getSystemLocations: () => shellState.systemLocations,
    getSystemLocationsError: () => shellState.systemLocationsError,
    getVolumes: () => shellState.volumes,
    getVolumesError: () => shellState.volumesError,
    getConnections: () => shellState.connections,
    getUnavailableLocations: () => shellState.unavailableLocations,
    getNativeIconLoader: () => shellState.nativeIconLoader,
    getThumbnailLoader: () => shellState.thumbnailLoader,
    getFinderTagsLoader: () => shellState.finderTagsLoader,
    getPlugins: () => shellState.plugins,
    getPlatform: () => shellState.platform,
    getKeybindingRuntime: () => shellState.keybindingRuntime,
    getRegisteredActions: () => shellState.registeredActions,
    getDraggedLocations: () => shellState.draggedLocations,
    getNativeDragOutSupported: () => shellState.nativeDragOutSupported,
    getNativeDropInProgress: () => shellState.nativeDropInProgress,
    getRenameRequest: (paneId) => shellState.renameRequests.get(paneId),
    getLocationEditRequest: (paneId) => shellState.locationEditRequests.get(paneId),
    getAppState: () => shellState.appState,
    getOperations: () =>
      Object.values(shellState.operations.byId).filter(
        (operation): operation is Operation => operation !== undefined,
      ),
    getContextMenu: () => shellState.contextMenu,
    clipboard: miscWiring.clipboard,
    getDirectories: () => shellState.directories,
    getSelections: () => shellState.selections,
    getSelectionDispatchers: () => shellState.selectionDispatchers,
    onSelectionAction: (key) => shellState.pendingCursorRestorations.delete(key),
    getSortedEntries: () => shellState.sortedEntries,
    getSortRequests: () => shellState.sortRequests,
    getCursorLoadTokens: () => shellState.cursorLoadTokens,
    getViewerByTab: () => shellState.viewerByTab,
    getEditorByPane: () => shellState.editorByPane,
    getEditorLinkHandlers: previewWiring.editorLinkHandlers,
    getPluginByPane: () => shellState.pluginByPane,
    getDiskUsageByTab: () => shellState.diskUsageByTab,
    getImageMapPane: imageMapWiring.imageMapPane,
    getKnowledgeSearchByTab: () => shellState.knowledgeSearchByTab,
    getThemesByTab: () => shellState.themesByTab,
    getKnowledgeAnswerRevision: () => shellState.knowledgeAnswerRevision,
    setConnections: (conns) => {
      shellState.connections = conns;
    },
    setConnectionsManagerOpen: (open) => {
      shellState.connectionsManagerOpen = open;
    },
    setAppState: (state) => {
      shellState.appState = state;
    },
    setQuickFilterOpen: (key, open) => {
      shellState.quickFilterOpen.set(key, open);
    },
    setDraggedLocations: (locs) => {
      shellState.draggedLocations = locs;
    },
    setNativeDragSourceInternal: (v) => {
      shellState.nativeDragSourceInternal = v;
    },
    setClipboardMessage: (msg) => {
      shellState.clipboardMessage = msg;
    },
    setMultiRenameOpen: (open) => {
      if (!open) shellState.dialogs.cancelMultiRename();
    },
    setMultiRenameEntries: (entries) => {
      shellState.dialogs.getState().multiRenameEntries = entries;
    },
    setMultiRenameLocation: (location) => {
      shellState.dialogs.getState().multiRenameLocation = location;
    },
    setMultiRenameExistingNames: (names) => {
      shellState.dialogs.getState().multiRenameExistingNames = names;
    },
    tabKey,
    effectiveSort,
    frontendSort,
    sortLabel,
    entriesSortedFor,
    entriesFilteredFor,
    quickFilterQueryFor,
    quickFilterOpenFor,
    quickFilterRecursiveFor,
    toggleQuickFilterScope,
    runRecursiveQuickFilter,
    advancedFilters: shellState.advancedFilters,
    contentSearchInitialQuery,
    searchQueryForLocationUri: (locationUri) =>
      shellState.findFilesQueriesByLocationUri.get(locationUri),
    searchFavouriteNameForLocationUri: (locationUri) => {
      const presentation = shellState.findFilesPresentationsByLocationUri.get(locationUri);
      return presentation?.label ?? presentation?.term;
    },
    workspaceErrorMessage,
    locationForPath: (current, path) => locationForPath(current, path, shellState.homeDirectory),
    activeDirectory,
    getNavigation: () => shellState.navigation,
    getSyncBrowsingState: () => shellState.syncedBrowsing?.state() ?? 'off',
    toggleSyncBrowsing: () => shellState.syncedBrowsing?.toggle(),
    getWorkspaceController: () => shellState.workspaceController,
    getOpsController: () => shellState.opsController,
    openSavedSearch: (saved) =>
      shellState.findFilesController.startSavedSearch(saved, 'currentPane'),
    openViewer: (paneId, entry, initialSearch, openMetadata, initialPage) =>
      previewWiring.openViewer(
        shellState.client,
        paneId,
        entry,
        initialSearch,
        openMetadata,
        initialPage,
      ),
    closeViewer: previewWiring.closeViewer,
    isDocumentSummaryAvailable: () => shellState.semanticAssistantAvailable,
    showEntryOnMap: imageMapWiring.showEntryOnMap,
    minifyGeo: (entry) => {
      shellState.minifyGeoRequest = { files: [{ location: entry.location, name: entry.name }] };
      m.redraw();
    },
    convertGeo: (entry) => {
      shellState.convertGeoRequest = { files: [{ location: entry.location, name: entry.name }] };
      m.redraw();
    },
    playQueueItems: (items, startId, mode, skipped) => {
      const player = shellState.playerController;
      if (player === undefined) return;
      if (items.length === 0) {
        toast({ html: t('player', 'noPlayableTracks') });
        return;
      }
      if (mode === 'play') player.playItems(items, startId);
      else if (mode === 'next') player.enqueueNext(items);
      else player.enqueue(items);
      if (mode !== 'play') toast({ html: t('player', 'queued', items.length) });
      if (skipped.remote > 0) toast({ html: t('player', 'playlistNetworkSkipped') });
      m.redraw();
    },
    openDocumentSummary: (_paneId, entry) => {
      if (!shellState.semanticAssistantAvailable) return;
      if (shellState.workspace === undefined) return;
      shellState.dialogs.openDocumentSummaryDialog({ workspaceId: shellState.workspace.id, entry });
      m.redraw();
    },
    openKnowledgeSearch: knowledgeWiring.openKnowledgeSearch,
    openKnowledgeAnswerSettings: () => settingsWiring.openSettingsDialog('semantic'),
    closeKnowledgeSearch: (paneId, tabId) =>
      shellState.tabController.performCloseTab(paneId, tabId),
    openThemePassage: knowledgeWiring.openThemePassage,
    searchThemeSources: (sourceIds, subject) =>
      knowledgeWiring.openKnowledgeSearchTab({
        semanticSourceIds: sourceIds,
        initialSubject: subject,
      }),
    openKnowledgeSource: knowledgeWiring.openKnowledgeSource,
    isKnowledgeSourceOpen: (evidence) =>
      [...shellState.viewerByTab.values()].some(
        (viewer) =>
          viewer.knowledgeSourceId === evidence.sourceId &&
          shellState.workspace?.panesById[viewer.paneId]?.activeTabId === viewer.tabId,
      ),
    closeEditor: previewWiring.closeEditor,
    updateLocationSettings,
    invokeActionById: (actionId, parameters, context) =>
      shellState.actionCommandController.invokeActionById(actionId, parameters, context),
    openContextMenu: (paneId, entries, x, y) =>
      shellState.actionCommandController.openContextMenu(paneId, entries, x, y),
    refetchAffectedPanes,
    replaceWorkspace,
    openDiskUsageFolder: opsWiring.openDiskUsageFolder,
    expandDiskUsageFolder: opsWiring.expandDiskUsageFolder,
    scanDiskUsageFolder: opsWiring.scanDiskUsageFolder,
    retryDiskUsage: (key) => {
      const entry = shellState.diskUsageByTab.get(key);
      if (entry !== undefined)
        opsWiring.startDiskUsageScan(entry.paneId, entry.tabId, entry.location);
    },
    stopDiskUsage: opsWiring.stopDiskUsage,
  };

  function replaceWorkspace(next: WorkspaceProjection): void {
    shellState.workspace = next;
    m.redraw();
  }

  /**
   * Applies a workspace projection fetched in reaction to a bare revision-bump notification from
   * the event stream (`backend-event-handler.ts`'s `'revision' in payload` branch) - the one path
   * that can carry *another* window's changes, since fm lets the same workspace be open in more
   * than one window at once and every window shares one event stream. `activePaneId` and each
   * pane's `activeTabId` describe which pane/tab has keyboard focus, which is inherently local to
   * a window, not something the other window's focus should override here - so this window's own
   * values are carried forward across the merge (falling back to the fetched ones only if the
   * fetched snapshot no longer has that pane/tab, e.g. the other window closed it).
   */
  function applyRemoteWorkspaceSnapshot(next: WorkspaceProjection): void {
    if (shellState.workspace === undefined || shellState.workspace.id !== next.id) {
      replaceWorkspace(next);
      return;
    }
    const localWorkspace = shellState.workspace;
    const activePaneId =
      next.panesById[localWorkspace.activePaneId] !== undefined
        ? localWorkspace.activePaneId
        : next.activePaneId;
    const panesById = Object.fromEntries(
      Object.entries(next.panesById).map(([paneId, pane]) => {
        const localActiveTabId = localWorkspace.panesById[paneId]?.activeTabId;
        const activeTabId =
          localActiveTabId !== undefined && pane.tabsById[localActiveTabId] !== undefined
            ? localActiveTabId
            : pane.activeTabId;
        return [paneId, activeTabId === pane.activeTabId ? pane : { ...pane, activeTabId }];
      }),
    );
    replaceWorkspace({ ...next, activePaneId, panesById });
  }

  async function updateLocationSettings(
    client: FileManagerClient,
    update: (settings: Settings) => Settings,
  ): Promise<void> {
    const pending = shellState.settingsUpdateQueue.then(async () => {
      if (shellState.currentSettings === undefined) {
        throw new ApiError(0, {
          code: 'settingsNotLoaded',
          message: 'Cannot update settings: settings have not loaded yet.',
        });
      }
      shellState.currentSettings = await client.updateSettings(update(shellState.currentSettings));
    });
    shellState.settingsUpdateQueue = pending.catch(() => undefined);
    return pending
      .catch((error: unknown) => {
        console.error('Failed to persist a settings update.', error);
        throw error;
      })
      .finally(() => m.redraw());
  }

  async function activatePane(client: FileManagerClient, paneId: PaneId): Promise<void> {
    if (shellState.workspace === undefined || shellState.workspace.activePaneId === paneId) {
      return;
    }
    const previousWorkspace = shellState.workspace;
    replaceWorkspace({ ...previousWorkspace, activePaneId: paneId });
    try {
      await dispatchWorkspaceCommand(
        client,
        {
          type: 'setActivePane',
          workspaceId: previousWorkspace.id,
          paneId,
          expectedRevision: previousWorkspace.revision,
        },
        replaceWorkspace,
      );
    } catch (error) {
      if (shellState.workspace?.revision === previousWorkspace.revision)
        replaceWorkspace(previousWorkspace);
      throw error;
    }
  }

  /** Writes this window's ephemeral workspace back into `targetWorkspaceId` - or, if omitted,
   * the named workspace it was forked from (creating one, on the first resync of a from-scratch
   * default session) - the only way a named workspace's tabs/panes/layout ever change, since
   * ephemeral windows are never kept in sync (ephemeral per-window workspaces spec follow-up).
   * Explicit action only: no autosave, no prompt on close. An explicit `targetWorkspaceId` (the
   * workspace switcher's per-row "Update" button) lets any saved workspace be replaced with this
   * session's current tabs, keeping that workspace's own name - not just the one this window
   * originally forked from. */
  async function resyncWorkspace(
    client: FileManagerClient,
    targetWorkspaceId?: WorkspaceId,
  ): Promise<void> {
    if (shellState.workspace === undefined) return;
    const target = await client
      .resyncWorkspace?.(shellState.workspace.id, targetWorkspaceId)
      .catch(() => undefined);
    if (target === undefined || shellState.workspace === undefined) return;
    // Any resync - the default source, a from-scratch session's first sync, or an explicit
    // "Update" onto a different saved workspace - relinks this window to whichever named
    // workspace it just synced into, mirroring what the backend already did to the persisted
    // ephemeral record, so a later default resync (or "New Window" from this window) keeps
    // targeting that same workspace.
    if (shellState.workspace.forkedFrom !== target.id) {
      shellState.workspace = { ...shellState.workspace, forkedFrom: target.id };
    }
    shellState.workspaceController.refreshWorkspaceSummaries();
    m.redraw();
  }

  function selectTab(client: FileManagerClient, paneId: PaneId, tabId: TabId): void {
    if (shellState.workspace?.panesById[paneId]?.activeTabId === tabId) {
      // Already the active tab - no tab switch, but still worth refreshing: the user is
      // deliberately revisiting this listing (e.g. clicking back onto it after an external change
      // like a browser download landed while it sat idle), so `activateTab` refreshes it too.
      void activatePane(client, paneId).catch(() => undefined);
      shellState.tabController.activateTab(paneId, tabId);
      return;
    }
    shellState.tabController.activateTab(paneId, tabId);
  }

  function updateLayout(client: FileManagerClient, layout: WorkspaceLayout): void {
    if (shellState.workspace === undefined) {
      return;
    }
    void dispatchWorkspaceCommand(
      client,
      {
        type: 'updateLayout',
        workspaceId: shellState.workspace.id,
        layout,
        expectedRevision: shellState.workspace.revision,
      },
      replaceWorkspace,
    ).catch(() => undefined);
  }

  function moveTab(
    client: FileManagerClient,
    sourcePaneId: PaneId,
    tabId: TabId,
    targetPaneId: PaneId,
    targetIndex: number,
  ): void {
    if (shellState.pluginByPane.get(sourcePaneId)?.tabId === tabId) return;
    const current = shellState.workspace;
    if (current === undefined) return;
    void dispatchWorkspaceCommand(
      client,
      {
        type: 'moveTab',
        workspaceId: current.id,
        sourcePaneId,
        tabId,
        targetPaneId,
        targetIndex,
        expectedRevision: current.revision,
      },
      (next) => {
        if (
          sourcePaneId !== targetPaneId &&
          next.panesById[sourcePaneId]?.tabsById[tabId] === undefined &&
          next.panesById[targetPaneId]?.tabsById[tabId] !== undefined
        ) {
          moveTabState(sourcePaneId, targetPaneId, tabId);
        }
        replaceWorkspace(next);
      },
    ).catch((error: unknown) => {
      toast({ html: workspaceErrorMessage(error, t('pane', 'moveTabFailed')) });
    });
  }

  const appDialogsContext: AppDialogsContext = {
    getHomeDirectory: () => shellState.homeDirectory,
    getOperationCentreVisible: () => shellState.workspace?.operationCentre.visible === true,
    toggleOperationCentre: opsWiring.toggleOperationCentre,
    getOperations: () => shellState.operations,
    setOperations: (next) => {
      shellState.operations = next;
    },
    getPendingConflict: () => shellState.pendingConflict,
    setPendingConflict: (conflict) => {
      shellState.pendingConflict = conflict;
    },
    getConnections: () => shellState.connections,
    setConnections: (conns) => {
      shellState.connections = conns;
    },
    getConnectionsManagerOpen: () => shellState.connectionsManagerOpen,
    setConnectionsManagerOpen: (open) => {
      shellState.connectionsManagerOpen = open;
    },
    getFindFilesOpen: () => shellState.findFilesOpen,
    getFindFilesRoot: () => shellState.findFilesRoot,
    getFindFilesError: () => shellState.findFilesError,
    getFindFilesSearchPending: () => shellState.pendingFindFilesStarts > 0,
    getCloseTabConfirmation: () => shellState.closeTabConfirmation,
    setCloseTabConfirmation: (conf) => {
      shellState.closeTabConfirmation = conf;
    },
    getDialogs: () => shellState.dialogs,
    getOperationConfirmation: () => shellState.pendingOperationConfirmation?.request,
    resolveOperationConfirmation: (decision) => {
      const pending = shellState.pendingOperationConfirmation;
      shellState.pendingOperationConfirmation = undefined;
      pending?.resolve(decision);
      m.redraw();
    },
    getPermanentDeleteRequest: () => shellState.pendingPermanentDelete?.sources,
    resolvePermanentDeleteRequest: (confirmed) => {
      const pending = shellState.pendingPermanentDelete;
      shellState.pendingPermanentDelete = undefined;
      pending?.resolve(confirmed);
      m.redraw();
    },
    getFormatSettings: () => shellState.currentEntryFormatSettings,
    getFindFilesController: () => shellState.findFilesController,
    getTabController: () => shellState.tabController,
    getOpsController: () => shellState.rawOpsController,
    openChangeAttributes: dialogWiring.openChangeAttributes,
    getActiveDirectoryLocation: () => activeDirectory()?.location,
    getActivePaneId: () => activeDirectory()?.paneId,
    navigateActiveLocation: async (location, preferredCursorName) => {
      const paneId = activeDirectory()?.paneId;
      if (paneId !== undefined)
        await shellState.navigation.navigate(paneId, location, preferredCursorName);
    },
    getFocusPane: () => shellState.focusPane,
    getSettings: () => shellState.currentSettings,
    getWorkspaceId: () => shellState.workspace?.id,
    updateSettings: (update) => updateLocationSettings(shellState.client, update),
    includeCurrentSemanticFolder: (workspaceId, location) => {
      shellState.semanticEnrolmentRequest = { workspaceId, location };
      shellState.semanticEnrolmentReturnToAsk = true;
      m.redraw();
    },
    openEditorForCreatedFile: (location, name) => {
      const active = activeDirectory();
      if (active === undefined) return;
      refetchAffectedPanes(active.paneId);
      previewWiring.openEditor(shellState.client, active.paneId, {
        id: crypto.randomUUID(),
        location,
        name,
        kind: 'file',
        hidden: false,
        readOnly: false,
        metadataRevision: 0,
      });
    },
    getFinderTagsLoader: () => shellState.finderTagsLoader,
    getRuntimeCapabilities: () => shellState.runtimeCapabilities,
    cancelAutoDismiss: opsWiring.cancelAutoDismiss,
    rememberDismissedOperation: opsWiring.rememberDismissedOperation,
    hasDismissedOperations: () => shellState.dismissedOperationIds.size > 0,
    showAllOperations: opsWiring.showAllOperations,
    getInterruptedTransfers: () => shellState.interruptedTransfers,
    getInterruptedTransferBusyId: () => shellState.interruptedTransferBusyId,
    getInterruptedTransferError: () => shellState.interruptedTransferError,
    restartInterruptedTransfer: opsWiring.restartInterruptedTransfer,
    discardInterruptedTransfer: opsWiring.discardInterruptedTransfer,
    refetchAffectedPanes,
    redraw: () => m.redraw(),
  };

  return {
    oninit: ({ attrs }) => {
      document.addEventListener('pointerdown', closeActivityOnOutsidePointer);
      shellState.client = attrs.client;
      void knowledgeWiring.refreshSemanticAssistantAvailability();
      void knowledgeWiring.refreshKnowledgeSearchAvailability();
      // Composition seam (task 0153, controller-registry.ts): every shell-lifetime controller is
      // constructed and torn down through this one registry instead of by-hand `let` +
      // `create*Controller(...)` + a matching teardown call hand-placed in `onremove`.
      const shellControllers = buildControllers({
        ops: {
          create: () => {
            shellState.rawOpsController = createOperationsController(attrs.client);
            return withActiveSourceGuard(
              withPermanentDeleteConfirmation(
                withOperationConfirmation(
                  shellState.rawOpsController,
                  () => shellState.currentSettings?.confirmFileOperations !== false,
                  (request) => {
                    shellState.pendingOperationConfirmation?.resolve(undefined);
                    return new Promise<OperationConfirmationDecision>((resolve) => {
                      shellState.pendingOperationConfirmation = { request, resolve };
                      m.redraw();
                    });
                  },
                  (sources, destination, signal) =>
                    attrs.client.findTransferConflicts(sources, destination, signal),
                ),
                (sources) => {
                  shellState.pendingPermanentDelete?.resolve(false);
                  return new Promise<boolean>((resolve) => {
                    shellState.pendingPermanentDelete = { sources, resolve };
                    m.redraw();
                  });
                },
              ),
              () =>
                Object.values(shellState.operations.byId).filter(
                  (operation) => operation !== undefined,
                ),
            );
          },
        },
        workspace: {
          create: () => createWorkspaceController(attrs.client, workspaceControllerContext),
        },
        tab: { create: () => createTabController(attrs.client, tabControllerContext) },
        settings: { create: () => createSettingsController(settingsControllerContext) },
        globalKeydown: {
          create: () => {
            const handler = createGlobalKeydownHandler(globalKeydownHandlerContext);
            document.addEventListener('keydown', handler);
            return handler;
          },
          dispose: (handler) => document.removeEventListener('keydown', handler),
        },
        findFiles: { create: () => createFindFilesController(findFilesControllerContext) },
        comparison: { create: () => createComparisonController(comparisonControllerContext) },
        syncProfiles: {
          create: () => {
            const controller = createSyncProfileController(syncProfileControllerContext);
            // Scheduled profiles only run while this window is open; each due profile prepares an
            // inspectable plan and honours its confirmation policy (task 0161).
            shellState.syncScheduleTimer = setInterval(() => {
              void controller.tick();
            }, SYNC_SCHEDULE_TICK_MS);
            return controller;
          },
          dispose: () => {
            if (shellState.syncScheduleTimer !== undefined)
              clearInterval(shellState.syncScheduleTimer);
            shellState.syncScheduleTimer = undefined;
          },
        },
        checksum: { create: () => createChecksumController(checksumControllerContext) },
        actionCommand: {
          create: () => createActionCommandController(actionCommandControllerContext),
        },
        paneContent: { create: () => createPaneContentBuilder(paneContentBuilderContext) },
        snapshots: {
          create: () =>
            createSnapshotController({
              getClient: () => attrs.client,
              getState: () => shellState.snapshotState,
              setState: (state) => {
                shellState.snapshotState = state;
              },
              toast: (message) => {
                toast({ html: message });
              },
              redraw: () => m.redraw(),
              refreshPanes: () => {
                for (const paneId of shellState.workspace?.paneOrder ?? [])
                  refetchAffectedPanes(paneId);
              },
            }),
          dispose: (controller) => controller.dispose(),
        },
        linkDoctor: {
          create: () =>
            createLinkDoctorController({
              getClient: () => attrs.client,
              getOpsController: () => shellState.opsController,
              getState: () => shellState.linkDoctorState,
              setState: (state) => {
                shellState.linkDoctorState = state;
              },
              toast: (message) => {
                toast({ html: message });
              },
              redraw: () => m.redraw(),
              openInEditor: (location) => {
                const paneId = shellState.workspace?.activePaneId;
                if (paneId === undefined) return;
                const name = decodeURIComponent(location.uri.split('/').pop() ?? location.uri);
                previewWiring.openEditor(attrs.client, paneId, {
                  id: crypto.randomUUID(),
                  location,
                  name,
                  kind: 'file',
                  hidden: false,
                  readOnly: false,
                  metadataRevision: 0,
                });
              },
              refreshPanes: () => {
                for (const paneId of shellState.workspace?.paneOrder ?? [])
                  refetchAffectedPanes(paneId);
              },
            }),
          dispose: (controller) => controller.dispose(),
        },
        musicDuplicates: {
          create: () =>
            createMusicDuplicatesController({
              getClient: () => attrs.client,
              getOpsController: () => shellState.opsController,
              getState: () => shellState.musicDuplicatesState,
              setState: (state) => {
                shellState.musicDuplicatesState = state;
              },
              toast: (message) => {
                toast({ html: message });
              },
              redraw: () => m.redraw(),
              refreshPanes: () => {
                for (const paneId of shellState.workspace?.paneOrder ?? [])
                  refetchAffectedPanes(paneId);
              },
            }),
          dispose: (controller) => controller.dispose(),
        },
        player: {
          create: () =>
            createPlayerController({
              client: attrs.client,
              prepareMedia: (location) => prepareMedia(location),
              getState: () => shellState.appState?.player ?? shellState.emptyPlayerState,
              update: (next: PlayerState) => {
                if (shellState.appState !== undefined)
                  shellState.appState = applyAppPatches(shellState.appState, playerPatch(next));
                m.redraw();
              },
            }),
          dispose: (controller) => controller.dispose(),
        },
        navigation: {
          create: () =>
            wrapSyncedBrowsing(
              createNavigationController({
                client: attrs.client,
                getWorkspace: () => shellState.workspace,
                replaceWorkspace: (next) => replaceWorkspace(next),
                onLocationVisited: (workspaceId, location) => {
                  shellState.unavailableLocations.delete(`${location.providerId}:${location.uri}`);
                  void updateLocationSettings(attrs.client, (settings) => ({
                    ...settings,
                    recentLocationsByWorkspace: {
                      ...settings.recentLocationsByWorkspace,
                      [workspaceId]: recordRecentLocation(
                        settings.recentLocationsByWorkspace[workspaceId] ?? [],
                        location,
                      ),
                    },
                  }));
                },
                onLocationUnavailable: (_workspaceId, location) => {
                  shellState.unavailableLocations.add(`${location.providerId}:${location.uri}`);
                  m.redraw();
                },
                requestArchivePassword,
                // Mirrors the same "on and the directory actually reports git status" gate
                // `pane-content-builder.ts` uses to decide whether to render the column - here it
                // decides whether to *ask the backend to compute* git status at all, so a hidden
                // column costs nothing server-side either. Read fresh on every request rather than
                // snapshotted, since `currentSettings` can change between navigations.
                getShowGitStatusColumn: () =>
                  shellState.currentSettings?.defaultColumns.includes('core.gitStatus') ?? false,
                updatePane: (paneId, tabId, view, preferredCursorName) => {
                  if (shellState.basketTabIds.has(tabId)) return;
                  const key = tabKey(paneId, tabId);
                  const previous = shellState.directories.get(key);
                  if (
                    previous !== undefined &&
                    (previous.location?.uri !== view.location?.uri ||
                      previous.location?.providerId !== view.location?.providerId)
                  ) {
                    shellState.pendingCursorRestorations.delete(key);
                  }
                  shellState.directories.set(
                    key,
                    respectSystemLocationReadOnly(view, shellState.systemLocations),
                  );
                  if (previous !== undefined && previous.location?.uri === view.location?.uri) {
                    reconcileSelectionAfterEntryChange(
                      paneId,
                      tabId,
                      previous.entries,
                      view.entries,
                    );
                  }
                  if (view.entries.length === 0) {
                    // Keep the synthetic ".." navigation row out of cursor/selection state. This
                    // also avoids a transient empty first page leaving ".." focused after the real
                    // entries arrive during workspace restoration.
                    shellState.selections.set(key, { selectedEntryIds: [] });
                    const pending = shellState.pendingCursorRestorations.get(key);
                    if (pending !== undefined) {
                      shellState.pendingCursorRestorations.set(key, {
                        ...pending,
                        fallbackCursorId: undefined,
                      });
                    }
                  } else if (
                    preferredCursorName !== undefined ||
                    shellState.selections.get(key)?.cursorEntryId === undefined ||
                    previous?.location?.uri !== view.location?.uri
                  ) {
                    // After `..` navigation, land the cursor back on the child directory
                    // just navigated away from instead of always the listing's first entry.
                    // Landing here (a fresh tab, a `..` navigation, switching back to a tab that
                    // was never given a cursor) only positions the keyboard cursor - it must never
                    // also select the entry. Selecting is a deliberate user action (click, keyboard
                    // select), not a side effect of simply looking at a directory or switching to
                    // it. `view.entries` is the raw, unsorted backend listing - the order the user
                    // actually sees (and arrow-keys through) comes from sorting/filtering it the
                    // same way `pane-content-builder.ts` does. The synthetic ".." row is
                    // deliberately excluded here: the cursor should land on the first real file
                    // below it, not on ".." itself (the empty-directory branch above already
                    // handles landing on ".." when there's nothing else to select).
                    const tab = shellState.workspace?.panesById[paneId]?.tabsById[tabId];
                    const sorted = entriesSortedFor(
                      key,
                      view.entries,
                      effectiveSort(tab?.view.sort ?? []),
                      tab?.view.foldersFirst ?? false,
                      tab?.location.uri.startsWith('search://') === true,
                    );
                    const filtered = entriesFilteredFor(key, sorted, quickFilterQueryFor(key, tab));
                    const preferredEntry = filtered.find(
                      (entry) => entry.name === preferredCursorName,
                    );
                    const firstEntry = preferredEntry ?? filtered[0];
                    shellState.selections.set(key, {
                      selectedEntryIds: [],
                      ...(firstEntry === undefined
                        ? {}
                        : { cursorEntryId: firstEntry.id, anchorEntryId: firstEntry.id }),
                    });
                  }
                  m.redraw();
                },
              }),
            ),
          dispose: (controller) => controller.dispose(),
        },
      });
      shellState.opsController = shellControllers.instances.ops;
      shellState.workspaceController = shellControllers.instances.workspace;
      shellState.tabController = shellControllers.instances.tab;
      shellState.settingsController = shellControllers.instances.settings;
      shellState.findFilesController = shellControllers.instances.findFiles;
      shellState.comparisonController = shellControllers.instances.comparison;
      shellState.syncProfileController = shellControllers.instances.syncProfiles;
      shellState.checksumController = shellControllers.instances.checksum;
      shellState.actionCommandController = shellControllers.instances.actionCommand;
      shellState.paneContentBuilder = shellControllers.instances.paneContent;
      shellState.playerController = shellControllers.instances.player;
      shellState.musicDuplicatesController = shellControllers.instances.musicDuplicates;
      shellState.linkDoctorController = shellControllers.instances.linkDoctor;
      shellState.snapshotController = shellControllers.instances.snapshots;
      shellState.navigation = shellControllers.instances.navigation;
      disposeShellControllers = shellControllers.dispose;
      shellState.keybindingRuntime = attrs.runtime === 'http' ? 'browser' : 'desktop';
      shellState.runtimeKind = attrs.runtime;
      shellState.windowTitleSink = dedupeWindowTitleSink(createWindowTitleSink(attrs.client));
      shellState.systemThemeQuery?.addEventListener(
        'change',
        settingsWiring.handleSystemThemeChange,
      );
      window.addEventListener('focus', handleWindowFocus);
      // A drive plugged in while Procyon stays focused raises no window event, so poll gently.
      cacheStatsTimer = window.setInterval(reportCacheStats, CACHE_STATS_INTERVAL_MS);
      volumePollTimer = window.setInterval(() => {
        if (document.visibilityState === 'visible') {
          void shellState.workspaceController.refreshVolumes();
        }
      }, VOLUME_POLL_INTERVAL_MS);
      document.addEventListener('visibilitychange', handleVisibilityChange);
      document.addEventListener('keydown', miscWiring.updateFunctionKeyModifiers);
      document.addEventListener('keyup', miscWiring.updateFunctionKeyModifiers);
      window.addEventListener('blur', miscWiring.resetFunctionKeyModifiers);
      shellState.appState = applyAppPatches(
        createInitialAppState(attrs.runtime),
        connectionPatch({ status: attrs.client.connection.get() }),
      );
      // Specification §26 keeps settings on the backend rather than in browser
      // storage, so the theme manager's own localStorage persistence stays off;
      // task 0030 restores the theme from the settings service instead.
      ThemeManager.setUseLocalStorage(false);
      ThemeManager.initialize(shellState.theme);
      void settingsWiring.loadSettings(attrs.client);
      void attrs.client
        .listActions()
        .then((actions) => {
          shellState.registeredActions = actions;
          m.redraw();
        })
        .catch(() => undefined);
      if (attrs.runtime === 'tauri') {
        void invoke('set_window_decorations', { decorations: false }).catch(() => undefined);
        if (!isWindowsTauriHost())
          void invoke('initialize_window_handle')
            .then(() => {
              try {
                // `new Channel()` synchronously reaches for `window.__TAURI_INTERNALS__` (unlike
                // plain `invoke()` calls, which are async and turn a missing host into a rejected
                // promise), so this needs its own guard for runtime:'tauri' test mounts with no
                // real Tauri host behind them.
                const nativeMenuActions = new Channel<{ id: string }>();
                nativeMenuActions.onmessage = (event) => {
                  dispatchNativeMenuAction(nativeMenuDispatchContext, event.id);
                  m.redraw();
                };
                void invoke('subscribe_native_menu_actions', { channel: nativeMenuActions })
                  .then(() => {
                    shellState.nativeMenuChannelReady = true;
                    m.redraw();
                  })
                  .catch(() => undefined);
              } catch {
                // No Tauri host available; the native menu bar is cosmetic desktop chrome.
              }
            })
            .catch(() => undefined);
      }
      void attrs.client
        .listPlugins()
        .then((listed) => {
          shellState.plugins = listed;
          if (shellState.currentSettings !== undefined)
            settingsWiring.applyIconTheme(shellState.currentSettings.iconTheme);
          m.redraw();
        })
        .catch(() => undefined);
      void shellState.workspaceController.loadWorkspace();
      opsWiring.loadInterruptedTransfers();
      void Promise.resolve()
        .then(() => attrs.client.listOperations())
        .then((listed) => {
          if (!shellState.removed) {
            // History is loaded from a PAST session - the user never watched these
            // run, so an auto-dismissible one (completed/cancelled/interrupted) would
            // only flash and vanish a few seconds later for no reason. Only surface
            // ones that still need attention (failed) or are still genuinely active.
            const relevant = listed.filter(
              (operation) =>
                !shouldAutoDismissOperation(operation) &&
                !shellState.dismissedOperationIds.has(operation.id),
            );
            shellState.operations = createOperationsState(relevant);
            m.redraw();
          }
        })
        .catch(() => undefined);
      shellState.unsubscribeConnection = attrs.client.connection.subscribe((status) => {
        if (shellState.appState !== undefined) {
          shellState.appState = applyAppPatches(shellState.appState, connectionPatch({ status }));
        }
        m.redraw();
      });
      shellState.unsubscribeResynchronise = attrs.client.onResynchronise(() =>
        refetchAffectedPanes(),
      );
      void attrs.client.subscribe(handleBackendEvent).then((unsubscribe) => {
        if (shellState.removed) unsubscribe();
        else shellState.unsubscribeEvents = unsubscribe;
      });
    },

    onremove: () => {
      document.removeEventListener('pointerdown', closeActivityOnOutsidePointer);
      shellState.removed = true;
      shellState.pendingOperationConfirmation?.resolve(undefined);
      shellState.pendingOperationConfirmation = undefined;
      shellState.pendingPermanentDelete?.resolve(false);
      shellState.pendingPermanentDelete = undefined;
      shellState.dialogs.getState().pendingArchiveCredential?.resolve(false);
      shellState.dialogs.clearArchiveCredential();
      shellState.systemThemeQuery?.removeEventListener(
        'change',
        settingsWiring.handleSystemThemeChange,
      );
      window.removeEventListener('focus', handleWindowFocus);
      if (volumePollTimer !== undefined) window.clearInterval(volumePollTimer);
      if (cacheStatsTimer !== undefined) window.clearInterval(cacheStatsTimer);
      document.removeEventListener('visibilitychange', handleVisibilityChange);
      document.removeEventListener('keydown', miscWiring.updateFunctionKeyModifiers);
      document.removeEventListener('keyup', miscWiring.updateFunctionKeyModifiers);
      window.removeEventListener('blur', miscWiring.resetFunctionKeyModifiers);
      if (shellState.operationFrame !== undefined) cancelAnimationFrame(shellState.operationFrame);
      for (const timer of shellState.autoDismissTimers.values()) clearTimeout(timer);
      shellState.autoDismissTimers.clear();
      shellState.workspaceRequest?.abort();
      shellState.unsubscribeEvents?.();
      shellState.unsubscribeNativeFileDrops?.();
      shellState.unsubscribeConnection?.();
      shellState.unsubscribeResynchronise?.();
      shellState.client.disconnect();
      disposeShellControllers?.();
      document.documentElement.style.removeProperty('--fm-font-size');
      document.documentElement.style.removeProperty('--fm-row-height');
    },

    view: ({ attrs }) => {
      const syncReview = shellState.syncProfileController.pendingReview();
      syncNativeMenu();
      syncWindowTitle();
      knowledgeWiring.refreshThemesForWorkspaceChange();
      if (shellState.treeSidebarOpen) treeWiring.syncDirectoryTreeToActiveLocation();
      shellState.currentEntryFormatSettings =
        attrs.entryFormatSettings ?? shellState.loadedEntryFormatSettings;
      const pendingDelete = Object.values(shellState.operations.byId).find(
        (operation) =>
          operation?.kind === 'delete' && operation.state === 'waitingForConflictResolution',
      );
      const activeOperations = Object.values(shellState.operations.byId).filter(
        (operation): operation is Operation =>
          operation !== undefined && isActiveOperation(operation),
      );
      const failedSemanticJobs = [...shellState.semanticJobs.values()].filter(
        (job) => job.stage === 'failed',
      );
      const activityCount = activeOperations.length + shellState.semanticJobs.size;
      const activityLabel =
        activityCount === 1
          ? t('shell', 'activitySingle')
          : t('shell', 'activityCount', { count: activityCount });
      const runningSemanticJobs = shellState.semanticJobs.size - failedSemanticJobs.length;
      const activityBusy = activeOperations.length > 0 || runningSemanticJobs > 0;
      const activityKinds = [
        ...new Set([
          ...activeOperations.map((operation) => operationKindLabel(operation.kind)),
          ...(runningSemanticJobs > 0 ? [t('shell', 'indexingActivity')] : []),
          ...(failedSemanticJobs.length > 0 ? [t('shell', 'indexingFailed')] : []),
        ]),
      ];
      const activityStatusLabel =
        activityKinds.length === 0
          ? activityLabel
          : `${activityLabel}: ${activityKinds.join(', ')}`;
      // The macOS overlay keeps native traffic lights while the toolbar draws the title.
      // The browser tab already provides a title; Windows has its own menu/title row.
      const isMacOverlay = shellState.runtimeKind === 'tauri' && shellState.platform === 'macos';
      const localisedActions = actionsWiring.keybindingActions();
      const keybindingContext = {
        scope: 'table' as const,
        platform: shellState.platform,
        runtime: attrs.runtime === 'http' ? ('browser' as const) : ('desktop' as const),
      };
      const footerBindings = footerFunctionKeyBindings(
        localisedActions,
        shellState.currentSettings?.keybindings ?? {},
        keybindingContext,
        (action) =>
          evaluateActionAvailability(
            action.id === 'core.edit' || action.id === 'core.view'
              ? {
                  ...action,
                  contextRequirements: {
                    ...action.contextRequirements,
                    featureAvailable: true,
                  },
                }
              : action,
            shellState.actionCommandController.commandAvailabilityContext(),
          ).available,
        shellState.functionKeyModifiers,
      );
      const liveShortcuts = getLiveBindings(
        localisedActions,
        shellState.currentSettings?.keybindings ?? {},
        keybindingContext,
      );
      const shortcutFor = (actionId: string): string | undefined =>
        liveShortcuts.find((binding) => binding.actionId === actionId)?.shortcut;
      const basketFocused =
        shellState.workspace !== undefined &&
        shellState.basketTabIds.has(
          shellState.workspace.panesById[shellState.workspace.activePaneId]?.activeTabId ?? '',
        );
      const basketF5 =
        basketWiring.basketVisible() && !basketFocused && shortcutFor('core.copy') === 'F5';
      return m(
        '.fm-app-shell',
        {
          'data-mac-titlebar-overlay': isMacOverlay ? 'true' : undefined,
          'data-mm-preset': 'compact-minimal',
        },
        [
          isWindowsTauriHost()
            ? m('.fm-windows-titlebar', [
                m('span.fm-windows-titlebar-label', { onmousedown: startWindowTitlebarDrag }, [
                  m('img.fm-windows-titlebar-icon', {
                    src: '/favicon-96x96.png',
                    alt: '',
                    'aria-hidden': 'true',
                  }),
                  t('shell', 'title'),
                ]),
                m(WindowsNativeMenu, {
                  spec: shellState.windowsNativeMenuSpec,
                  onAction: (id) => dispatchNativeMenuAction(nativeMenuDispatchContext, id),
                  onRole: (role) => {
                    const win = getCurrentWindow();
                    if (role === 'minimize') void win.minimize();
                    if (role === 'zoom') void win.toggleMaximize();
                    if (role === 'quit') void win.close();
                    if (role === 'about') {
                      shellState.aboutDialogOpen = true;
                      m.redraw();
                    }
                  },
                }),
                // Fills the flex gap left by the menu bar's natural width so most of the bar is
                // draggable, without tagging the whole titlebar - which would make the menu's
                // absolutely-positioned dropdown popups (its DOM descendants) draggable too and
                // swallow clicks on their items into a window-drag gesture instead.
                m('.fm-windows-titlebar-spacer', { onmousedown: startWindowTitlebarDrag }),
                m('.fm-windows-titlebar-controls', [
                  m(
                    'button',
                    {
                      type: 'button',
                      'aria-label': t('shell', 'minimize'),
                      onclick: () => void getCurrentWindow().minimize(),
                    },
                    windowControlIcon('M1 6.5h10'),
                  ),
                  m(
                    'button',
                    {
                      type: 'button',
                      'aria-label': t('shell', 'maximize'),
                      onclick: () => void getCurrentWindow().toggleMaximize(),
                    },
                    windowControlIcon('M1.5 1.5h9v9h-9z'),
                  ),
                  m(
                    'button.fm-windows-titlebar-close',
                    {
                      type: 'button',
                      'aria-label': t('button', 'close'),
                      onclick: () => void getCurrentWindow().close(),
                    },
                    windowControlIcon('M1.5 1.5l9 9M10.5 1.5l-9 9'),
                  ),
                ]),
              ])
            : undefined,
          m('.fm-workspace-toolbar', { 'data-tauri-drag-region': isMacOverlay ? '' : undefined }, [
            isMacOverlay
              ? m('.fm-mac-toolbar-brand', { onmousedown: startWindowTitlebarDrag }, [
                  m('img.fm-mac-toolbar-icon', {
                    src: '/favicon-96x96.png',
                    alt: '',
                    'aria-hidden': 'true',
                  }),
                  m('span', t('shell', 'title')),
                ])
              : undefined,
            m('.fm-navigation-controls', { 'aria-label': t('shell', 'activePaneNavigation') }, [
              tooltip(
                t('shell', 'back'),
                m(
                  IconButton,
                  {
                    disabled:
                      activeDirectory() === undefined ||
                      shellState.workspace?.panesById[shellState.workspace.activePaneId]?.tabsById[
                        shellState.workspace.panesById[shellState.workspace.activePaneId]
                          ?.activeTabId ?? ''
                      ]?.canNavigateBack !== true,
                    'aria-label': t('shell', 'back'),
                    onclick: () =>
                      void shellState.navigation.back(shellState.workspace?.activePaneId ?? ''),
                  },
                  arrowLeftIcon(),
                ),
              ),
              tooltip(
                t('shell', 'forward'),
                m(
                  IconButton,
                  {
                    disabled:
                      activeDirectory() === undefined ||
                      shellState.workspace?.panesById[shellState.workspace.activePaneId]?.tabsById[
                        shellState.workspace.panesById[shellState.workspace.activePaneId]
                          ?.activeTabId ?? ''
                      ]?.canNavigateForward !== true,
                    'aria-label': t('shell', 'forward'),
                    onclick: () =>
                      void shellState.navigation.forward(shellState.workspace?.activePaneId ?? ''),
                  },
                  arrowRightIcon(),
                ),
              ),
              tooltip(
                miscWiring.labelWithShortcut(
                  t('shell', 'parentDirectory'),
                  shortcutFor('core.parent'),
                ),
                m(
                  IconButton,
                  {
                    disabled: activeDirectory() === undefined,
                    'aria-label': t('shell', 'parentDirectory'),
                    onclick: () =>
                      void shellState.navigation.parent(shellState.workspace?.activePaneId ?? ''),
                  },
                  cornerLeftUpIcon(),
                ),
              ),
            ]),
            m('.fm-toolbar-separator', {
              role: 'separator',
              'aria-orientation': 'vertical',
            }),
            m('.fm-search-controls', [
              tooltip(
                miscWiring.labelWithShortcut(
                  t('shell', 'findFiles'),
                  shortcutFor('core.findFiles'),
                ),
                m(
                  IconButton,
                  {
                    disabled: activeDirectory() === undefined,
                    'aria-label': t('shell', 'findFiles'),
                    onclick: () => {
                      shellState.findFilesController.openFindFiles();
                    },
                  },
                  searchIcon(),
                ),
              ),
              shellState.knowledgeSearchAvailable || !shellState.knowledgeSearchChecked
                ? tooltip(
                    miscWiring.labelWithShortcut(
                      !shellState.knowledgeSearchChecked
                        ? t('knowledgeSearch', 'checkingAvailability')
                        : t(
                            'knowledgeSearch',
                            shellState.semanticAssistantAvailable
                              ? 'openWithAskTitle'
                              : 'openTitle',
                          ),
                      shortcutFor('client.searchKnowledge'),
                    ),
                    m(
                      IconButton,
                      {
                        className: 'fm-knowledge-search-trigger',
                        disabled: !shellState.knowledgeSearchAvailable,
                        'aria-label': !shellState.knowledgeSearchChecked
                          ? t('knowledgeSearch', 'checkingAvailability')
                          : t(
                              'knowledgeSearch',
                              shellState.semanticAssistantAvailable
                                ? 'openWithAskTitle'
                                : 'openTitle',
                            ),
                        onclick: knowledgeWiring.openKnowledgeSearch,
                      },
                      contentSearchIcon(),
                    ),
                  )
                : undefined,
              shellState.themesAvailable && shellState.themesWorkspaceIndexed
                ? tooltip(
                    miscWiring.labelWithShortcut(
                      t('themes', 'openTitle'),
                      shortcutFor('client.exploreThemes'),
                    ),
                    m(
                      IconButton,
                      {
                        className: 'fm-themes-trigger',
                        'aria-label': t('themes', 'openTitle'),
                        onclick: knowledgeWiring.openThemes,
                      },
                      themeMapIcon(),
                    ),
                  )
                : undefined,
            ]),
            m('.fm-toolbar-separator', {
              role: 'separator',
              'aria-orientation': 'vertical',
            }),
            tooltip(
              t('basket', 'add'),
              m(
                IconButton,
                {
                  className: 'fm-basket-add-button',
                  disabled: basketWiring.basketSourcePaneId() === undefined,
                  'aria-label': t('basket', 'add'),
                  onclick: basketWiring.collectSelection,
                },
                plusIcon(),
              ),
            ),
            tooltip(
              basketWiring.basketTabLocation() === undefined
                ? t('basket', 'open')
                : t('basket', 'close'),
              m(
                IconButton,
                {
                  className: 'fm-basket-open-button',
                  disabled: shellState.workspace === undefined,
                  'aria-label':
                    basketWiring.basketTabLocation() === undefined
                      ? t('basket', 'open')
                      : t('basket', 'close'),
                  'aria-pressed': String(basketWiring.basketTabLocation() !== undefined),
                  onclick: basketWiring.toggleBasket,
                },
                basketWiring.basketTabLocation() === undefined ? basketIcon() : basketOpenIcon(),
              ),
            ),
            m('.fm-toolbar-separator', {
              role: 'separator',
              'aria-orientation': 'vertical',
            }),
            tooltip(
              miscWiring.labelWithShortcut(
                t('shell', 'comparePanes'),
                shortcutFor('core.compareDirectories'),
              ),
              m(
                IconButton,
                {
                  disabled:
                    activeDirectory() === undefined ||
                    (shellState.workspace?.paneOrder.length ?? 0) < 2,
                  'aria-label': t('shell', 'comparePanes'),
                  onclick: () =>
                    shellState.comparisonController.startComparison('sizeAndTimestamp'),
                },
                compareIcon(),
              ),
            ),
            tooltip(
              miscWiring.labelWithShortcut(
                t('shell', 'commandPalette'),
                `${shellState.platform === 'macos' ? 'Cmd' : 'Ctrl'}+P`,
              ),
              m(
                IconButton,
                {
                  className: 'fm-command-palette-trigger',
                  disabled: shellState.registeredActions.length === 0,
                  'aria-label': t('shell', 'commandPalette'),
                  onclick: () => {
                    shellState.commandPaletteOpen = true;
                  },
                },
                commandIcon(),
              ),
            ),
            m('.fm-toolbar-spacer', {
              'aria-hidden': 'true',
              'data-tauri-drag-region': isMacOverlay ? '' : undefined,
            }),
            tooltip(
              t('shell', 'workspaceSwitcherLabel', {
                name: shellState.workspace?.name ?? t('shell', 'none'),
              }),
              m(
                IconButton,
                {
                  className: 'fm-workspace-switcher-button',
                  'aria-label': t('shell', 'workspaceSwitcherLabel', {
                    name: shellState.workspace?.name ?? t('shell', 'none'),
                  }),
                  onclick: () => {
                    if (shellState.workspaceDisclosureElement !== undefined) {
                      shellState.workspaceDisclosureElement.open =
                        !shellState.workspaceDisclosureElement.open;
                    }
                  },
                },
                layoutGridIcon(),
              ),
            ),
            m(
              'details.fm-workspace-disclosure',
              {
                oncreate: ({ dom }) => {
                  shellState.workspaceDisclosureElement = dom as HTMLDetailsElement;
                },
                onremove: () => {
                  shellState.workspaceDisclosureElement = undefined;
                },
              },
              [
                m('summary.fm-disclosure-summary-hidden'),
                m('.fm-workspace-switcher-backdrop', {
                  onclick: (event: MouseEvent) => {
                    const disclosure = (event.currentTarget as HTMLElement).closest('details');
                    if (disclosure instanceof HTMLDetailsElement) disclosure.open = false;
                  },
                }),
                m(
                  '.fm-workspace-switcher-panel',
                  { role: 'dialog', 'aria-label': t('shell', 'workspaceSwitcher') },
                  [
                    m('.fm-workspace-switcher-heading', [
                      m('strong', t('shell', 'workspaceSwitcher')),
                      m(
                        'button',
                        {
                          type: 'button',
                          'aria-label': t('shell', 'closeWorkspaces'),
                          onclick: (event: MouseEvent) => {
                            const disclosure = (event.currentTarget as HTMLElement).closest(
                              'details',
                            );
                            if (disclosure instanceof HTMLDetailsElement) disclosure.open = false;
                          },
                        },
                        closeIcon(),
                      ),
                    ]),
                    m(WorkspaceSwitcher, {
                      summaries: sortWorkspaceSummaries(shellState.workspaceSummaries),
                      // An ephemeral window's own id is never listed here (ephemeral workspaces
                      // are excluded from the switcher) - the row that should read as "current"
                      // is the named workspace it was forked from, if any. A non-ephemeral
                      // window (the main/dock window) already is that named workspace.
                      activeWorkspaceId:
                        shellState.workspace === undefined
                          ? undefined
                          : shellState.workspace.ephemeral
                            ? shellState.workspace.forkedFrom
                            : shellState.workspace.id,
                      error: shellState.workspaceActionError,
                      onSwitch: (workspaceId) => {
                        void switchWorkspace(workspaceId);
                      },
                      onCreate: () => shellState.workspaceController.createWorkspaceAction(),
                      onRename: (workspaceId, name) =>
                        shellState.workspaceController.renameWorkspaceAction(workspaceId, name),
                      onDelete: (workspaceId) =>
                        shellState.workspaceController.deleteWorkspaceAction(workspaceId),
                      ...(shellState.client.openWorkspaceWindow === undefined
                        ? {}
                        : {
                            onOpenInNewWindow: (workspaceId: WorkspaceId) => {
                              void shellState.client.openWorkspaceWindow?.(workspaceId);
                              if (shellState.workspaceDisclosureElement !== undefined) {
                                shellState.workspaceDisclosureElement.open = false;
                              }
                            },
                          }),
                      ...(shellState.client.resyncWorkspace === undefined ||
                      shellState.workspace === undefined
                        ? {}
                        : {
                            onUpdate: (workspaceId: WorkspaceId) => {
                              void resyncWorkspace(shellState.client, workspaceId);
                            },
                          }),
                    }),
                  ],
                ),
              ],
            ),
            m('.fm-toolbar-separator', {
              role: 'separator',
              'aria-orientation': 'vertical',
            }),
            m(
              '.fm-activity-anchor',
              {
                onkeydown: (event: KeyboardEvent) => {
                  if (event.key !== 'Escape' || !shellState.activityOpen) return;
                  event.preventDefault();
                  shellState.activityOpen = false;
                  document.querySelector<HTMLButtonElement>('.fm-activity-count')?.focus();
                },
              },
              [
                tooltip(
                  t('shell', 'operationCentre'),
                  m(
                    IconButton,
                    {
                      className: 'fm-operation-centre-button',
                      disabled: shellState.workspace === undefined,
                      'aria-label': t('shell', 'operationCentre'),
                      'aria-pressed': String(
                        shellState.workspace?.operationCentre.visible === true,
                      ),
                      onclick: () => {
                        shellState.activityOpen = false;
                        opsWiring.toggleOperationCentre();
                      },
                    },
                    listIcon(),
                  ),
                ),
                activityCount > 0
                  ? m(
                      'button.fm-activity-count',
                      {
                        type: 'button',
                        className: [
                          failedSemanticJobs.length > 0 ? 'fm-activity-failed' : '',
                          activityBusy ? 'fm-activity-busy' : '',
                        ]
                          .filter(Boolean)
                          .join(' '),
                        'aria-label': activityStatusLabel,
                        'aria-expanded': String(shellState.activityOpen),
                        'aria-controls': 'fm-activity-popover',
                        title: activityStatusLabel,
                        onclick: () => {
                          shellState.activityOpen = !shellState.activityOpen;
                        },
                      },
                      [
                        m('span.fm-activity-icon', activityIcon({ size: 16 })),
                        m('span.fm-activity-number', activityCount),
                      ],
                    )
                  : undefined,
                shellState.activityOpen && activityCount > 0
                  ? m(
                      '.fm-activity-popover#fm-activity-popover',
                      {
                        role: 'region',
                        'aria-label': t('shell', 'showActivity'),
                      },
                      [
                        m('strong', { key: 'heading' }, activityLabel),
                        ...activeOperations.map((operation) =>
                          m(
                            'button.fm-activity-item',
                            {
                              type: 'button',
                              key: operation.id,
                              onclick: () => {
                                shellState.activityOpen = false;
                                opsWiring.setOperationCentreVisible(true);
                                requestAnimationFrame(() => {
                                  document
                                    .querySelector<HTMLElement>(
                                      `.fm-operation[data-operation-id="${operation.id}"]`,
                                    )
                                    ?.focus();
                                });
                              },
                            },
                            [
                              m('span', operationKindLabel(operation.kind)),
                              m('span', itemProgressSummary(operation)),
                              m('span.fm-activity-target', t('shell', 'openOperation')),
                            ],
                          ),
                        ),
                        ...[...shellState.semanticJobs.values()].map((job) =>
                          m(
                            'button.fm-activity-item',
                            {
                              type: 'button',
                              key: job.jobId,
                              onclick: () => {
                                shellState.activityOpen = false;
                                if (job.stage === 'failed')
                                  shellState.semanticJobs.delete(job.jobId);
                                settingsWiring.openSettingsDialog('semantic');
                              },
                            },
                            [
                              m(
                                'span',
                                job.stage === 'failed'
                                  ? job.errors > 0
                                    ? t('shell', 'indexingFailures', job.errors)
                                    : t('shell', 'indexingFailed')
                                  : t('shell', 'indexingActivity'),
                              ),
                              job.stage === 'failed'
                                ? undefined
                                : m(
                                    'span',
                                    job.completed === 1
                                      ? t('shell', 'indexingScannedOne')
                                      : t('shell', 'indexingScanned', { count: job.completed }),
                                  ),
                              m('span.fm-activity-target', t('shell', 'openSemanticStatus')),
                            ],
                          ),
                        ),
                      ],
                    )
                  : undefined,
              ],
            ),
            shellState.semanticInstallation === undefined
              ? undefined
              : m(
                  'button.fm-semantic-install-status',
                  {
                    type: 'button',
                    className: `fm-semantic-install-${shellState.semanticInstallation.state}`,
                    title: shellState.semanticInstallation.message,
                    'aria-label': shellState.semanticInstallation.message,
                    onclick: () => settingsWiring.openSettingsDialog('semantic'),
                  },
                  activityIcon({ size: 16 }),
                ),
            m('.fm-toolbar-separator', {
              role: 'separator',
              'aria-orientation': 'vertical',
            }),
            tooltip(
              miscWiring.labelWithShortcut(
                t('shell', 'openSettings'),
                `${shellState.platform === 'macos' ? 'Cmd' : 'Ctrl'}+,`,
              ),
              m(
                IconButton,
                {
                  className: 'fm-settings-button',
                  'aria-label': t('shell', 'settings'),
                  onclick: () => {
                    if (shellState.settingsDisclosureElement === undefined) return;
                    const openStarted = perfStart();
                    shellState.settingsInitialSection = 'appearance';
                    shellState.settingsDisclosureElement.open =
                      !shellState.settingsDisclosureElement.open;
                    shellState.settingsDialogOpen = shellState.settingsDisclosureElement.open;
                    if (shellState.settingsDialogOpen) {
                      perfEndAfterRender('settings-open', openStarted, { via: 'button' });
                    }
                    if (
                      !shellState.settingsDialogOpen &&
                      shellState.currentSettings !== undefined
                    ) {
                      settingsWiring.applyAppearance(shellState.currentSettings);
                      settingsWiring.settingsClosed();
                    }
                    m.redraw();
                  },
                },
                settingsIcon(),
              ),
            ),
            tooltip(
              t('menu', 'help'),
              m(
                IconButton,
                {
                  className: 'fm-settings-button fm-help-trigger',
                  'aria-label': t('walkthroughs', 'title'),
                  onclick: () => {
                    shellState.walkthroughsOpen = true;
                  },
                },
                helpIcon(),
              ),
            ),
            m(
              'details.fm-settings-disclosure',
              {
                oncreate: ({ dom }) => {
                  shellState.settingsDisclosureElement = dom as HTMLDetailsElement;
                },
                onremove: () => {
                  shellState.settingsDisclosureElement = undefined;
                },
                ontoggle: (event: Event) => {
                  const open = (event.currentTarget as HTMLDetailsElement).open;
                  if (shellState.settingsDialogOpen === open) return;
                  shellState.settingsDialogOpen = open;
                  if (!open && shellState.currentSettings !== undefined) {
                    settingsWiring.applyAppearance(shellState.currentSettings);
                    settingsWiring.settingsClosed();
                  }
                  m.redraw();
                },
              },
              [
                m('summary.fm-disclosure-summary-hidden'),
                m(
                  '.fm-settings-editor',
                  {
                    role: 'dialog',
                    'aria-label': t('shell', 'settings'),
                    onclick: (event: MouseEvent) => {
                      if (event.target === event.currentTarget) {
                        settingsWiring.closeSettingsDialog();
                      }
                    },
                  },
                  [
                    m('.fm-settings-editor-panel.fm-settings-editor-panel--fixed-footer', [
                      m('.fm-settings-editor-heading', [
                        m('strong', t('shell', 'settings')),
                        m(
                          'button',
                          {
                            type: 'button',
                            'aria-label': t('shell', 'closeSettings'),
                            onclick: () => settingsWiring.closeSettingsDialog(),
                          },
                          closeIcon(),
                        ),
                      ]),
                      shellState.currentSettings === undefined
                        ? m('p', t('shell', 'loading'))
                        : shellState.settingsDialogOpen
                          ? m(SettingsEditor, {
                              client: attrs.client,
                              settings: shellState.currentSettings,
                              initialSection: shellState.settingsInitialSection,
                              ...(shellState.workspace === undefined
                                ? {}
                                : { activeWorkspaceId: shellState.workspace.id }),
                              ...(activeDirectory()?.location === undefined
                                ? {}
                                : { activeLocation: activeDirectory()?.location }),
                              actions: actionsWiring.localisedRegisteredActions(),
                              platform: shellState.platform,
                              runtime: shellState.keybindingRuntime,
                              plugins: shellState.plugins,
                              onOpenLocation: (location: Location) => {
                                const paneId = activeDirectory()?.paneId;
                                settingsWiring.closeSettingsDialog();
                                if (paneId !== undefined)
                                  void shellState.navigation.navigate(paneId, location);
                              },
                              onPreview: (draft: Settings) => {
                                settingsWiring.applyAppearance(draft);
                                m.redraw();
                              },
                              onSave: async (draft: Settings) => {
                                const showHiddenChanged =
                                  shellState.currentSettings !== undefined &&
                                  shellState.currentSettings.showHiddenFiles !==
                                    draft.showHiddenFiles;
                                await updateLocationSettings(attrs.client, (latest) => ({
                                  ...draft,
                                  multiRenamePresets: latest.multiRenamePresets,
                                  filterPresets: latest.filterPresets,
                                  syncProfiles: latest.syncProfiles,
                                }));
                                settingsWiring.applyAppearance(draft);
                                settingsWiring.closeSettingsDialog();
                                if (showHiddenChanged) {
                                  void settingsWiring.applyShowHiddenFilesToAllTabs(
                                    attrs.client,
                                    draft.showHiddenFiles,
                                  );
                                }
                              },
                              onCancel: () => {
                                if (shellState.currentSettings !== undefined)
                                  settingsWiring.applyAppearance(shellState.currentSettings);
                                settingsWiring.closeSettingsDialog();
                              },
                              onTogglePlugin: (pluginId: PluginId, enabled: boolean) =>
                                attrs.client.setPluginEnabled(pluginId, enabled),
                              onRequestPluginLogs: (
                                pluginId: PluginId,
                              ): Promise<readonly PluginLogEntry[]> =>
                                attrs.client.getPluginLogs(pluginId),
                              onSemanticStatusChange: (status) => {
                                void knowledgeWiring.refreshSemanticAssistantAvailability(status);
                                void knowledgeWiring.refreshKnowledgeSearchAvailability();
                              },
                              semanticInstallInProgress:
                                shellState.semanticInstallation?.state === 'installing',
                              onSemanticInstallationChange: (state, message) => {
                                shellState.semanticInstallation = { state, message };
                                if (state !== 'installing') {
                                  const text = document.createElement('span');
                                  text.textContent = message;
                                  toast({
                                    html: text.outerHTML,
                                    className: `fm-semantic-install-${state}`,
                                  });
                                }
                                m.redraw();
                              },
                            })
                          : undefined,
                    ]),
                  ],
                ),
              ],
            ),
          ]),
          shellState.walkthroughsOpen
            ? m(WalkthroughPanel, {
                selected: shellState.selectedWalkthrough,
                onSelect: (id) => {
                  shellState.selectedWalkthrough = id;
                },
                onClose: () => {
                  shellState.walkthroughsOpen = false;
                  document.querySelector<HTMLButtonElement>('.fm-help-trigger')?.focus();
                },
              })
            : undefined,
          shellState.walkthroughInvitationVisible
            ? m(
                'aside.fm-walkthrough-invitation',
                { 'aria-label': t('walkthroughs', 'invitation') },
                [
                  m('span.fm-walkthrough-invitation-copy', t('walkthroughs', 'invitation')),
                  m(
                    'button.fm-walkthrough-invitation-action',
                    {
                      type: 'button',
                      onclick: () => {
                        shellState.walkthroughInvitationVisible = false;
                        shellState.selectedWalkthrough = 'basics';
                        shellState.walkthroughsOpen = true;
                      },
                    },
                    t('walkthroughs', 'watch'),
                  ),
                  m(
                    'button.fm-walkthrough-invitation-dismiss',
                    {
                      type: 'button',
                      'aria-label': t('walkthroughs', 'dismiss'),
                      onclick: () => {
                        shellState.walkthroughInvitationVisible = false;
                      },
                    },
                    m(
                      'svg',
                      {
                        viewBox: '0 0 16 16',
                        width: 16,
                        height: 16,
                        fill: 'none',
                        stroke: 'currentColor',
                        'stroke-width': 2,
                        'aria-hidden': 'true',
                      },
                      m('path', { d: 'M4 4l8 8M12 4l-8 8' }),
                    ),
                  ),
                ],
              )
            : undefined,
          m('main.fm-workspace', [
            (() => {
              if (!shellState.treeSidebarOpen || shellState.treeRootLocation === undefined)
                return undefined;
              const treeRoot = shellState.treeRootLocation;
              const activeLocationUri = activeDirectory()?.location.uri;
              return m.fragment({}, [
                m('button.fm-directory-tree-backdrop', {
                  type: 'button',
                  tabindex: -1,
                  'aria-hidden': 'true',
                  onclick: treeWiring.toggleDirectoryTree,
                }),
                m('.fm-directory-tree-sidebar', [
                  m('.fm-directory-tree-header', [
                    m('span', t('tree', 'directoryTree')),
                    m(
                      'button.fm-directory-tree-close',
                      {
                        type: 'button',
                        'aria-label': t('tree', 'toggleSidebar'),
                        onclick: treeWiring.toggleDirectoryTree,
                      },
                      closeIcon({ size: 13 }),
                    ),
                  ]),
                  m(DirectoryTree, {
                    root: { location: treeRoot, name: treeRootName(treeRoot) },
                    state: shellState.treeState,
                    ...(activeLocationUri === undefined ? {} : { activeLocationUri }),
                    onToggleExpand: (location: Location) => {
                      treeWiring.toggleTreeNode(location);
                      m.redraw();
                    },
                    onActivate: (location: Location) => {
                      const active = activeDirectory();
                      if (active === undefined) return;
                      void shellState.navigation?.navigate(active.paneId, location);
                    },
                    onDragStart: (location, event) => {
                      shellState.draggedLocations = [location];
                      event.dataTransfer?.setData('application/x-fm-locations', 'internal');
                      if (event.dataTransfer != null) event.dataTransfer.effectAllowed = 'copyMove';
                    },
                    onDragEnd: () => {
                      shellState.draggedLocations = [];
                    },
                    ...(shellState.nativeDragOutSupported
                      ? {
                          onPointerDragStart: (location: Location) => {
                            shellState.draggedLocations = [location];
                          },
                          onPointerDragCancel: () => {
                            shellState.draggedLocations = [];
                          },
                          pointerDragEffect: (event: DropModifiers) =>
                            operationForDrop(shellState.platform, event),
                          onPointerDragOut: (location: Location) => {
                            shellState.draggedLocations = [location];
                            shellState.nativeDragSourceInternal = true;
                            return shellState.client
                              .startNativeDrag([location])
                              .catch((error: unknown) => {
                                shellState.draggedLocations = [];
                                shellState.nativeDragSourceInternal = false;
                                shellState.clipboardMessage = workspaceErrorMessage(
                                  error,
                                  t('shell', 'nativeDragFailed'),
                                );
                                m.redraw();
                              });
                          },
                        }
                      : {}),
                    onDragOver: (location, event) => {
                      const validation = validateDropTarget(
                        shellState.draggedLocations,
                        location,
                        treeWiring.treeDropWritable(location),
                      );
                      if (!validation.ok) return false;
                      if (event.dataTransfer != null)
                        event.dataTransfer.dropEffect = operationForDrop(
                          shellState.platform,
                          event,
                        );
                      return true;
                    },
                    onDrop: treeWiring.dropOnTree,
                    onTabOut: (direction) => {
                      const paneOrder = shellState.workspace?.paneOrder;
                      if (paneOrder === undefined || paneOrder.length === 0) return;
                      const targetPaneId =
                        direction === 1 ? paneOrder[0] : paneOrder[paneOrder.length - 1];
                      if (targetPaneId !== undefined)
                        globalKeydownHandlerContext.focusPane(targetPaneId);
                    },
                    registerFocus: (focus) => {
                      shellState.focusDirectoryTree = focus;
                    },
                  } satisfies DirectoryTreeAttrs),
                ]),
              ]);
            })(),
            shellState.workspace === undefined
              ? m('.fm-workspace-loading', shellState.workspaceError ?? t('shell', 'loading'))
              : m(WorkspaceLayoutView, {
                  workspace: shellState.workspace,
                  paneContent: (paneId) => {
                    const currentWorkspace = shellState.workspace;
                    const tabId = currentWorkspace?.panesById[paneId]?.activeTabId;
                    if (
                      currentWorkspace !== undefined &&
                      tabId !== undefined &&
                      shellState.basketTabIds.has(tabId)
                    ) {
                      basketWiring.syncBasketVirtualDirectory(paneId, tabId, currentWorkspace.id);
                    }
                    const content = shellState.paneContentBuilder(
                      attrs.client,
                      attrs.entryFormatSettings ?? shellState.loadedEntryFormatSettings,
                      paneId,
                    );
                    const basketTabs = currentWorkspace?.panesById[paneId]?.tabOrder.filter((id) =>
                      shellState.basketTabIds.has(id),
                    );
                    const isActiveBasketTab =
                      currentWorkspace !== undefined &&
                      tabId !== undefined &&
                      shellState.basketTabIds.has(tabId);
                    const titledContent =
                      basketTabs === undefined || basketTabs.length === 0
                        ? content
                        : {
                            ...content,
                            groupByParent: basketTabs.some((id) => id === tabId),
                            ...(isActiveBasketTab
                              ? {
                                  location: basketWiring.basketVirtualLocation(currentWorkspace.id),
                                  breadcrumbLabel: t('basket', 'title'),
                                }
                              : {}),
                            viewerTitles: new Map([
                              ...(content.viewerTitles ?? []),
                              ...basketTabs.map((id): [TabId, string] => [
                                id,
                                t('basket', 'title'),
                              ]),
                            ]),
                          };
                    return titledContent;
                  },
                  onActivatePane: (paneId) =>
                    void activatePane(attrs.client, paneId).catch(() => undefined),
                  onUpdateLayout: (layout) => updateLayout(attrs.client, layout),
                  onSelectTab: (paneId, tabId) => selectTab(attrs.client, paneId, tabId),
                  onCloseTab: (paneId, tabId) =>
                    shellState.tabController.requestCloseTab(paneId, tabId),
                  onNewTab: (paneId) => shellState.tabController.openTab(paneId),
                  onMoveTab: (sourcePaneId, tabId, targetPaneId, targetIndex) =>
                    moveTab(attrs.client, sourcePaneId, tabId, targetPaneId, targetIndex),
                  onFocusTerminal: () => shellState.focusTerminal?.() ?? false,
                  onFocusViewer: () => {
                    const paneId = miscWiring.openViewerPaneId();
                    if (paneId === undefined) return false;
                    miscWiring.focusViewer(paneId);
                    m.redraw();
                    return true;
                  },
                  onPaneCycleBoundary: () => {
                    if (!shellState.treeSidebarOpen) return false;
                    shellState.focusDirectoryTree?.();
                    return true;
                  },
                  registerFlush: (flush) => {
                    shellState.flushPendingLayoutUpdate = flush;
                  },
                  registerFocusPane: (focus) => {
                    shellState.focusPane = focus;
                  },
                  searchPresentationForLocationUri: (uri) =>
                    shellState.findFilesPresentationsByLocationUri.get(uri),
                  onRefreshSearch: (uri, paneId) =>
                    shellState.findFilesController.refreshSearch(uri, paneId),
                }),
            // Checksum and duplicate panels sit below the panes, visible only
            // while their job/scan is tracked (task 0077).
            shellState.checksumState.jobId !== undefined &&
              m(ChecksumResultsView, {
                algorithms: shellState.checksumState.algorithms,
                entries: shellState.checksumState.entries,
                totalEntries: shellState.checksumState.totalEntries,
                isComplete: shellState.checksumState.isComplete,
                isCancelled: shellState.checksumState.isCancelled,
                ...(shellState.checksumState.verification === undefined
                  ? {}
                  : { verification: shellState.checksumState.verification }),
                ...(shellState.checksumState.error === undefined
                  ? {}
                  : { error: shellState.checksumState.error }),
                onCopy: (algorithm) => {
                  void shellState.checksumController.copyChecksums(algorithm).then((content) => {
                    if (content !== undefined) void navigator.clipboard?.writeText(content);
                  });
                },
                ...(shellState.checksumState.savedTo === undefined
                  ? {}
                  : { savedTo: shellState.checksumState.savedTo }),
                suggestedFileName: (algorithm) =>
                  shellState.checksumController.suggestedFileName(algorithm),
                onSave: (algorithm, fileName) => {
                  void shellState.checksumController.saveChecksumFile(algorithm, fileName);
                },
                onVerify: (content) => shellState.checksumController.verifyAgainst(content),
                onCancel: () => shellState.checksumController.cancelChecksums(),
                onClose: () => shellState.checksumController.closeChecksums(),
              }),
            shellState.duplicateState.scanId !== undefined &&
              m(DuplicateReviewView, {
                groups: shellState.duplicateState.groups,
                isComplete: shellState.duplicateState.isComplete,
                isCancelled: shellState.duplicateState.isCancelled,
                warningsCount: shellState.duplicateState.warningsCount,
                selectedUris: shellState.duplicateState.selectedUris,
                totalReclaimableBytes: totalReclaimableBytes(shellState.duplicateState),
                ...(shellState.duplicateState.error === undefined
                  ? {}
                  : { error: shellState.duplicateState.error }),
                isLastCopy: (uri) => wouldDeleteEveryCopy(shellState.duplicateState, uri),
                onToggle: (uri) => shellState.checksumController.toggleDuplicateSelection(uri),
                onDeleteSelected: () => shellState.checksumController.deleteSelectedDuplicates(),
                onCancel: () => shellState.checksumController.cancelDuplicateScan(),
                onClose: () => shellState.checksumController.closeDuplicates(),
              }),
            shellState.snapshotState.jobId !== undefined &&
              shellState.snapshotController !== undefined &&
              m(SnapshotView, {
                state: shellState.snapshotState,
                onCancel: () => void shellState.snapshotController?.cancel(),
                onClose: () => shellState.snapshotController?.close(),
              }),
            shellState.linkDoctorState.scanId !== undefined &&
              shellState.linkDoctorController !== undefined &&
              m(LinkDoctorView, {
                state: shellState.linkDoctorState,
                onFindReplacement: (problem) =>
                  void shellState.linkDoctorController?.findReplacement(problem),
                onRepoint: (problem, candidate) =>
                  void shellState.linkDoctorController?.repointSymlink(problem, candidate),
                onOpenSource: (problem) => shellState.linkDoctorController?.openSource(problem),
                onCancel: () => void shellState.linkDoctorController?.cancel(),
                onClose: () => shellState.linkDoctorController?.close(),
              }),
            shellState.musicDuplicatesState.scanId !== undefined &&
              shellState.musicDuplicatesController !== undefined &&
              m(MusicDuplicatesView, {
                state: shellState.musicDuplicatesState,
                isLastCopy: (uri) => shellState.musicDuplicatesController?.isLastCopy(uri) ?? false,
                onToggle: (uri) => shellState.musicDuplicatesController?.toggle(uri),
                onSelectSuggested: () =>
                  shellState.musicDuplicatesController?.selectSuggestedRemovals(),
                onClearSelection: () => shellState.musicDuplicatesController?.clearSelection(),
                onRemoveSelected: () => void shellState.musicDuplicatesController?.removeSelected(),
                onCancel: () => void shellState.musicDuplicatesController?.cancel(),
                onClose: () => shellState.musicDuplicatesController?.close(),
              }),
          ]),
          shellState.runtimeKind === 'tauri'
            ? m(TerminalDrawer, {
                open: isTerminalVisible(
                  shellState.openTerminalTabKeys,
                  treeWiring.activeTerminalTabKey(),
                ),
                tabKey: treeWiring.activeTerminalTabKey(),
                location: activeDirectory()?.location,
                client: tauriTerminalClient,
                onToggle: globalKeydownHandlerContext.toggleTerminal,
                onSwitchPane: () => {
                  if (shellState.workspace === undefined) return;
                  const index = shellState.workspace.paneOrder.indexOf(
                    shellState.workspace.activePaneId,
                  );
                  const nextPaneId =
                    shellState.workspace.paneOrder[
                      (index + 1) % shellState.workspace.paneOrder.length
                    ];
                  if (nextPaneId !== undefined) shellState.focusPane?.(nextPaneId);
                },
                onCycleTab: (direction) => {
                  if (shellState.workspace === undefined) return;
                  const paneId = shellState.workspace.activePaneId;
                  shellState.tabController.cycleTab(paneId, direction);
                  shellState.focusPane?.(paneId);
                },
                onFocusFolder: () => {
                  if (shellState.workspace !== undefined)
                    shellState.focusPane?.(shellState.workspace.activePaneId);
                },
                registerFocus: (focus) => {
                  shellState.focusTerminal = focus;
                },
                registerDisposeTab: (dispose) => {
                  shellState.disposeTerminalTab = dispose;
                },
              })
            : undefined,
          shellState.clipboardMessage === undefined
            ? undefined
            : m('.fm-clipboard-message', { role: 'alert' }, [
                m('span.fm-clipboard-message-text', shellState.clipboardMessage),
                m(
                  'button.fm-clipboard-dismiss.btn-flat',
                  {
                    type: 'button',
                    'aria-label': t('button', 'dismiss'),
                    title: t('button', 'dismiss'),
                    onclick: () => {
                      shellState.clipboardMessage = undefined;
                    },
                  },
                  m('i.material-icons', { 'aria-hidden': 'true' }, 'close'),
                ),
              ]),
          m(CommandPalette, {
            open: shellState.commandPaletteOpen,
            actions: actionsWiring.actionsWithFavourites(),
            recency: shellState.commandPaletteRecency,
            context: shellState.actionCommandController.actionContext(),
            availabilityContext: shellState.actionCommandController.commandAvailabilityContext(),
            platform: shellState.platform,
            onClose: () => {
              shellState.commandPaletteOpen = false;
            },
            onInvoke: shellState.actionCommandController.invokePaletteAction,
          }),
          m(ModalPanel, {
            title: t('ragAsk', 'includeFolderTitle'),
            className: 'fm-dense-modal fm-semantic-enrolment-modal',
            isOpen: shellState.semanticEnrolmentRequest !== undefined,
            closeOnEsc: true,
            onToggle: (open: boolean) => {
              if (!open) {
                const returnToAsk = shellState.semanticEnrolmentReturnToAsk;
                shellState.semanticEnrolmentRequest = undefined;
                shellState.semanticEnrolmentReturnToAsk = false;
                if (returnToAsk) knowledgeWiring.openRagAsk();
              }
            },
            ...(shellState.semanticEnrolmentRequest === undefined
              ? {}
              : {
                  description: m(
                    '.fm-semantic-enrolment-content',
                    m(SemanticFolderEnrolmentPrompt, {
                      client: attrs.client,
                      workspaceId: shellState.semanticEnrolmentRequest.workspaceId,
                      location: shellState.semanticEnrolmentRequest.location,
                      onEnrolled: () => {
                        shellState.semanticEnrolmentRequest = undefined;
                        shellState.semanticEnrolmentReturnToAsk = false;
                        knowledgeWiring.openRagAsk();
                      },
                      onCancel: () => {
                        shellState.semanticEnrolmentRequest = undefined;
                      },
                    }),
                  ),
                }),
          }),
          m(DirectoryContextMenu, {
            open: shellState.contextMenu !== undefined,
            x: shellState.contextMenu?.x ?? 0,
            y: shellState.contextMenu?.y ?? 0,
            actions:
              shellState.contextMenu === undefined
                ? []
                : menuActionsForContext(
                    [
                      ...actionsWiring.localisedRegisteredActions(),
                      ...actionsWiring.mediaQueueActions(),
                    ],
                    shellState.actionCommandController.commandAvailabilityContext(
                      shellState.contextMenu.entries,
                      shellState.contextMenu.paneId,
                    ),
                  ),
            onClose: () => {
              shellState.contextMenu = undefined;
            },
            ...(shellState.contextMenu !== undefined &&
            [...shellState.pluginByPane].some(
              ([paneId, panel]) =>
                paneId !== shellState.contextMenu?.paneId &&
                shellState.workspace?.panesById[paneId]?.activeTabId === panel.tabId,
            )
              ? {
                  placementBounds: () =>
                    document
                      .querySelector<HTMLElement>(
                        `[data-pane-id="${shellState.contextMenu?.paneId}"]`,
                      )
                      ?.getBoundingClientRect(),
                }
              : {}),
            onInvoke: shellState.actionCommandController.invokeContextMenuAction,
            platform: shellState.platform,
            ...(shellState.platform === 'macos' &&
            shellState.runtimeKind === 'tauri' &&
            shellState.contextMenu?.entries.length === 1 &&
            shellState.contextMenu.entries[0]?.location.providerId === 'local' &&
            shellState.contextMenu.entries[0]?.kind === 'file'
              ? {
                  openWithSubmenu: {
                    load: () => {
                      const location = shellState.contextMenu?.entries[0]?.location;
                      if (location === undefined) {
                        return Promise.reject(
                          new Error('Open With selection is no longer available'),
                        );
                      }
                      return shellState.client.listOpenWithApplications(location);
                    },
                    iconFor: (path: string) => {
                      const url = new URL('file:///');
                      url.pathname = path;
                      return shellState.client.getFileIcon(url.href);
                    },
                    onChoose: (applicationPath: string) => {
                      const location = shellState.contextMenu?.entries[0]?.location;
                      if (location === undefined) {
                        toast({ html: t('contextMenu', 'openWithFailed') });
                        return;
                      }
                      void shellState.client
                        .openWithApplication(location, applicationPath)
                        .catch((error: unknown) => {
                          console.error('Failed to open with selected application', error);
                          toast({ html: t('contextMenu', 'openWithFailed') });
                        });
                    },
                    onOther: () => {
                      const location = shellState.contextMenu?.entries[0]?.location;
                      if (location === undefined) {
                        toast({ html: t('contextMenu', 'openWithFailed') });
                        return;
                      }
                      void shellState.client
                        .openWithAnyApplication(location)
                        .catch((error: unknown) => {
                          console.error('Failed to open application picker', error);
                          toast({ html: t('contextMenu', 'openWithFailed') });
                        });
                    },
                  },
                }
              : {}),
            ...(shellState.platformContextMenuSupported &&
            shellState.contextMenu !== undefined &&
            shellState.contextMenu.entries.length > 0 &&
            shellState.contextMenu.entries.every(
              (entry) => entry.location.providerId === 'local',
            ) &&
            (shellState.platform === 'macos' || shellState.platform === 'windows')
              ? {
                  platformSubmenu: {
                    title:
                      shellState.platform === 'macos'
                        ? t('contextMenu', 'services')
                        : t('contextMenu', 'sendTo'),
                    onOpen: () => {
                      const locations =
                        shellState.contextMenu?.entries.map((entry) => entry.location) ?? [];
                      void shellState.client.showPlatformContextMenu(locations).catch(() => {
                        toast({ html: t('contextMenu', 'platformMenuFailed') });
                      });
                    },
                  },
                }
              : {}),
          }),
          m(ShortcutsHelpDialog, {
            open: shellState.shortcutsHelpOpen,
            actions: actionsWiring.localisedRegisteredActions(),
            keybindings: shellState.currentSettings?.keybindings ?? {},
            platform: shellState.platform,
            runtime: shellState.keybindingRuntime,
            onClose: () => {
              shellState.shortcutsHelpOpen = false;
            },
          }),
          m(ModalPanel, {
            title: t('menu', 'about'),
            description: m('.fm-about-dialog', [
              m('img.fm-about-icon', { src: '/favicon-96x96.png', alt: '' }),
              m('p', t('shell', 'title')),
              m('p.fm-about-version', t('shell', 'aboutVersion', { version: packageJson.version })),
              m(
                'p.fm-about-developer',
                t('shell', 'aboutDeveloper', { developer: 'Erik Vullings' }),
              ),
              m(
                'p.fm-about-repository',
                m(
                  'a',
                  {
                    href: 'https://github.com/erikvullings/procyon',
                    target: '_blank',
                    rel: 'noopener noreferrer',
                  },
                  t('shell', 'aboutRepository'),
                ),
              ),
            ]),
            isOpen: shellState.aboutDialogOpen,
            closeOnEsc: true,
            onToggle: (open: boolean) => {
              if (!open) shellState.aboutDialogOpen = false;
            },
          }),
          m(AppUpdateDialog, {
            client: attrs.client,
            ...(shellState.availableAppUpdate === undefined
              ? {}
              : { update: shellState.availableAppUpdate }),
            onLater: () => {
              shellState.availableAppUpdate = undefined;
            },
          }),
          m(InsufficientSpaceDialog, {
            shortfall: Object.values(shellState.operations.byId)
              .filter((operation): operation is Operation => operation !== undefined)
              .filter((operation) => !shellState.answeredSpaceShortfalls.has(operation.id))
              .map((operation) =>
                spaceShortfallOf(operation, shellState.operations.failuresById[operation.id]),
              )
              .find((shortfall) => shortfall !== undefined),
            onCancel: (shortfall) => {
              shellState.answeredSpaceShortfalls.add(shortfall.operation.id);
            },
            onProceed: (shortfall) => {
              shellState.answeredSpaceShortfalls.add(shortfall.operation.id);
              void attrs.client
                .startOperation(requestSkippingSpaceCheck(shortfall.operation))
                .then(() => {
                  shellState.operations = dismissOperation(
                    shellState.operations,
                    shortfall.operation.id,
                  );
                  m.redraw();
                })
                .catch((error: unknown) => {
                  toast({ html: errorText(error) });
                });
            },
          }),
          ...renderAppDialogs(attrs.client, pendingDelete, appDialogsContext),
          m(SaveSnapshotDialog, {
            request: shellState.saveSnapshotRequest,
            onConfirm: (fileName, withChecksums) => {
              const request = shellState.saveSnapshotRequest;
              shellState.saveSnapshotRequest = undefined;
              if (request === undefined) return;
              void shellState.snapshotController?.start({
                type: 'create',
                root: request.root,
                destination: childLocation(request.directory, fileName),
                withChecksums,
              });
            },
            onCancel: () => {
              shellState.saveSnapshotRequest = undefined;
            },
          }),
          m(MinifyGeoDialog, {
            request: shellState.minifyGeoRequest,
            client: attrs.client,
            onApply: dialogWiring.confirmMinifyGeo,
            onCancel: () => {
              shellState.minifyGeoRequest = undefined;
              m.redraw();
            },
          }),
          m(FileDiffDialog, {
            request: shellState.fileDiffRequest,
            client: attrs.client,
            onClose: () => {
              shellState.fileDiffRequest = undefined;
              m.redraw();
            },
          }),
          m(SyncProfilesDialog, {
            open: shellState.syncProfilesDialogOpen,
            controller: shellState.syncProfileController,
            paneLocations: syncProfilePaneLocations,
            onClose: () => {
              shellState.syncProfilesDialogOpen = false;
              m.redraw();
            },
          }),
          m(SyncPlanDialog, {
            open: syncReview?.open === true,
            items: syncReview?.plan.items ?? [],
            applying: syncReview?.applying === true,
            ...(syncReview?.error === undefined ? {} : { error: syncReview.error }),
            onApply: (items) => {
              void shellState.syncProfileController.confirmReview(items);
            },
            onCancel: () => {
              void shellState.syncProfileController.dismissReview();
            },
          }),
          m(SplitFileDialog, {
            request: shellState.splitFileRequest,
            onSplit: dialogWiring.confirmSplitFile,
            onCancel: () => {
              shellState.splitFileRequest = undefined;
              m.redraw();
            },
          }),
          m(CombineFilesDialog, {
            request: shellState.combineFilesRequest,
            client: attrs.client,
            onCombine: dialogWiring.confirmCombineFiles,
            onCancel: () => {
              shellState.combineFilesRequest = undefined;
              m.redraw();
            },
          }),
          m(ConvertGeoDialog, {
            request: shellState.convertGeoRequest,
            client: attrs.client,
            onConvert: dialogWiring.confirmConvertGeo,
            onCancel: () => {
              shellState.convertGeoRequest = undefined;
              m.redraw();
            },
          }),
          m(ChangeAttributesDialog, {
            request: shellState.changeAttributesRequest,
            client: attrs.client,
            onApply: dialogWiring.confirmChangeAttributes,
            onCancel: () => {
              shellState.changeAttributesRequest = undefined;
              m.redraw();
            },
          }),
          m(EditImageMetadataDialog, {
            request: shellState.editImageMetadataRequest,
            onApply: dialogWiring.confirmEditImageMetadata,
            onCancel: () => {
              shellState.editImageMetadataRequest = undefined;
              m.redraw();
            },
          }),
          m(SavePlaylistDialog, {
            request: shellState.savePlaylistRequest,
            onConfirm: dialogWiring.confirmSavePlaylist,
            onCancel: () => {
              shellState.savePlaylistRequest = undefined;
            },
          }),
          shellState.appState !== undefined && shellState.playerController !== undefined
            ? m(MiniPlayer, {
                state: shellState.appState.player,
                controller: shellState.playerController,
              })
            : undefined,
          m(
            '.fm-function-key-bar',
            m(
              '.fm-function-key-list',
              footerBindings.map((binding) => {
                const collect =
                  basketF5 && binding.actionId === 'core.copy' && binding.shortcut === 'F5';
                const available = collect
                  ? basketWiring.basketSourcePaneId() !== undefined
                  : binding.actionAvailable;
                return m(
                  'button.fm-function-key',
                  {
                    key: binding.actionId,
                    type: 'button',
                    disabled: !available,
                    onclick: available
                      ? () => miscWiring.invokeFunctionKeyShortcut(binding.shortcut)
                      : undefined,
                  },
                  `${binding.key} ${collect ? t('basket', 'addShort') : miscWiring.functionKeyTitle(binding)}`,
                );
              }),
            ),
          ),
        ],
      );
    },
  };
};
