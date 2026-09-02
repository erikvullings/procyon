import { Channel, invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import m, { type FactoryComponent } from 'mithril';
import { IconButton, ModalPanel, type Theme, ThemeManager, toast } from 'mithril-materialized';

import packageJson from '../../package.json' with { type: 'json' };

import type { FileManagerClient } from '../api/client/file-manager-client';
import {
  activityIcon,
  arrowLeftIcon,
  arrowRightIcon,
  closeIcon,
  commandIcon,
  compareIcon,
  cornerLeftUpIcon,
  layoutGridIcon,
  listIcon,
  searchIcon,
  settingsIcon,
} from '../components/tabler-icons';
import { tooltip } from '../components/tooltip';
import {
  type ActionCommandController,
  type ActionCommandControllerContext,
  createActionCommandController,
} from '../features/actions/action-command-controller';
import {
  type ChecksumController,
  type ChecksumControllerContext,
  createChecksumController,
} from '../features/checksums/checksum-controller';
import { ChecksumResultsView } from '../features/checksums/checksum-results-view';
import {
  type ChecksumState,
  type DuplicateState,
  initialChecksumState,
  initialDuplicateState,
  totalReclaimableBytes,
  wouldDeleteEveryCopy,
} from '../features/checksums/checksum-state';
import { DuplicateReviewView } from '../features/checksums/duplicate-review-view';
import { emptyClipboard } from '../features/clipboard/clipboard';
import { CommandPalette } from '../features/command-palette/command-palette';
import {
  evaluateActionAvailability,
  menuActionsForContext,
} from '../features/commands/availability';
import { ContextMenu as DirectoryContextMenu } from '../features/commands/context-menu';
import {
  type ComparisonController,
  type ComparisonControllerContext,
  createComparisonController,
} from '../features/comparison/comparison-controller';
import {
  type ComparisonState,
  differingEntryIds,
  initialComparisonState,
} from '../features/comparison/comparison-state';
import { DiagnosticsViewComponent } from '../features/diagnostics/diagnostics-view';
import { type AppDialogsContext, renderAppDialogs } from '../features/dialogs/app-dialogs';
import { createDialogUIController } from '../features/dialogs/dialog-ui-controller';
import type { FinderTagsLoader } from '../features/directory-table/finder-tags-loader';
import type { NativeIconLoader } from '../features/directory-table/native-icon-loader';
import type { ThumbnailLoader } from '../features/directory-table/thumbnail-loader';
import { DirectoryTree, type DirectoryTreeAttrs } from '../features/directory-tree/directory-tree';
import {
  ancestorChain,
  createTreeChildrenState,
  type TreeChildrenState,
  withChildren,
  withError,
  withExpanded,
  withLoading,
} from '../features/directory-tree/directory-tree-state';
import type { DiskUsageViewState } from '../features/disk-usage/disk-usage-view';
import {
  createFileEditorController,
  type FileEditorController,
  type FileEditorState,
} from '../features/editor/file-editor-controller';
import {
  DEFAULT_ENTRY_FORMAT_SETTINGS,
  type EntryFormatSettings,
} from '../features/entry-formatting/entry-formatting';
import {
  type BackendEventContext,
  createBackendEventHandler,
} from '../features/events/backend-event-handler';
import { recordRecentLocation } from '../features/favourites/favourites';
import {
  createGlobalKeydownHandler,
  type GlobalKeydownContext,
} from '../features/keybindings/global-keydown-handler';
import { ShortcutsHelpDialog } from '../features/keybindings/shortcuts-help-dialog';
import {
  dispatchNativeMenuAction,
  type NativeMenuDispatchContext,
} from '../features/native-menu/native-menu-dispatch';
import { buildNativeMenuSpec, type NativeMenuTab } from '../features/native-menu/native-menu-spec';
import { WindowsNativeMenu } from '../features/native-menu/windows-native-menu';
import {
  createNavigationController,
  type NavigationController,
  type PaneDirectoryView,
} from '../features/navigation/navigation';
import { rootLocationFor } from '../features/navigation/root-location';
import {
  createOperationsState,
  dismissOperation,
  mergeOperationHistory,
  shouldAutoDismissOperation,
} from '../features/operations/operation-state';
import {
  createOperationsController,
  type OperationsController,
} from '../features/operations/operations-controller';
import { isParentEntry, withParentEntry } from '../features/panes/parent-entry';
import {
  createTabController,
  type TabController,
  type TabControllerContext,
} from '../features/panes/tab-controller';
import {
  createFileViewerController,
  type FileViewerController,
  type FileViewerState,
} from '../features/preview/file-viewer-controller';
import { filterEntries } from '../features/quick-filter/quick-filter';
import {
  createFindFilesController,
  type FindFilesController,
  type FindFilesControllerContext,
} from '../features/search/find-files-controller';
import type { FindFilesSearchParams } from '../features/search/find-files-dialog';
import type { SearchPresentation } from '../features/search/search-presentation';
import type { SelectionPlatform } from '../features/selection/keybindings';
import {
  emptySelection,
  getSelectedEntries,
  getSelectedEntriesOrCursor,
  reduceSelection,
  type SelectionState,
} from '../features/selection/selection';
import {
  createSettingsController,
  type SettingsController,
  type SettingsControllerContext,
} from '../features/settings/settings-controller';
import { SettingsEditor } from '../features/settings/settings-editor';
import {
  type SortColumn,
  type SortModel,
  sortEntries,
  sortEntriesResponsive,
} from '../features/sorting/sorting';
import { tauriTerminalClient } from '../features/terminal/terminal-client';
import { TerminalDrawer } from '../features/terminal/terminal-drawer';
import { isTerminalVisible } from '../features/terminal/terminal-state';
import { dispatchWorkspaceCommand } from '../features/workspace/dispatch-workspace-command';
import {
  createPaneContentBuilder,
  type PaneContentContext,
} from '../features/workspace/pane-content-builder';
import {
  createWorkspaceController,
  type WorkspaceController,
  type WorkspaceControllerContext,
} from '../features/workspace/workspace-controller';
import {
  pathFromUri,
  WorkspaceLayoutView,
  type WorkspacePaneContent,
} from '../features/workspace/workspace-layout';
import { sortWorkspaceSummaries } from '../features/workspace/workspace-manager';
import { WorkspaceSwitcher } from '../features/workspace/workspace-switcher';
import { actionTitle, t } from '../i18n';
import {
  type FunctionKeyModifiers,
  footerFunctionKeyBindings,
  hasPrimaryModifier,
  type KeybindingRuntime,
} from '../keybindings/dispatcher';
import type {
  ActionDescriptor,
  BackendEvent,
  Connection,
  DirectoryDelta,
  EntrySummary,
  Location,
  NativeMenuSpec,
  Operation,
  OperationConflict,
  OperationId,
  PaneId,
  PluginDescriptor,
  PluginId,
  PluginLogEntry,
  ScanDiskUsageResult,
  SearchExecutionMode,
  SearchQuery,
  Settings,
  SortDescriptor,
  SystemLocation,
  TabId,
  TabProjection,
  Volume,
  WorkspaceId,
  WorkspaceLayout,
  WorkspaceProjection,
  WorkspaceSummary,
} from '../models';
import {
  type AppState,
  applyAppPatches,
  cacheContentMatchesPatch,
  clipboardPatch,
  connectionPatch,
  createInitialAppState,
  deleteQuickFilterDraftPatch,
  setQuickFilterDraftPatch,
} from '../state';
import type { RuntimeKind } from '../utilities/runtime';
import { buildControllers } from './controller-registry';

/** Attributes of the application shell. */
export interface AppShellAttrs {
  /** Transport this build talks to, resolved from `VITE_RUNTIME`. */
  runtime: RuntimeKind;
  /** Transport-neutral client selected once by the application bootstrap. */
  client: FileManagerClient;
  /** Settings-owned presentation formats; task 0030 supplies these at bootstrap. */
  entryFormatSettings?: EntryFormatSettings;
}

interface DiskUsageTabEntry {
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

const DEFAULT_THEME: Theme = 'auto';

/** Applies host-detected mount access metadata to a directory view. */
export function respectSystemLocationReadOnly(
  view: PaneDirectoryView,
  locations: readonly SystemLocation[],
): PaneDirectoryView {
  if (
    view.location === undefined ||
    !locations.some(
      ({ location, readOnly }) =>
        readOnly === true &&
        location.providerId === view.location?.providerId &&
        (location.uri === view.location.uri ||
          view.location.uri.startsWith(
            location.uri.endsWith('/') ? location.uri : `${location.uri}/`,
          )),
    )
  ) {
    return view;
  }
  return { ...view, writable: false };
}

const DISMISSED_OPERATIONS_STORAGE_KEY = 'fm.dismissedOperationIds';
const MAX_DISMISSED_OPERATIONS = 500;

function loadDismissedOperationIds(): Set<OperationId> {
  try {
    const raw = globalThis.localStorage?.getItem(DISMISSED_OPERATIONS_STORAGE_KEY);
    if (raw === null) return new Set();
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) return new Set();
    return new Set(parsed.filter((value): value is OperationId => typeof value === 'string'));
  } catch {
    return new Set();
  }
}

function persistDismissedOperationIds(ids: ReadonlySet<OperationId>): void {
  try {
    const recent = [...ids].slice(-MAX_DISMISSED_OPERATIONS);
    globalThis.localStorage?.setItem(DISMISSED_OPERATIONS_STORAGE_KEY, JSON.stringify(recent));
  } catch {
    // localStorage can be unavailable in some runtimes; in-memory dismissal still works.
  }
}

/**
 * Converts a displayed breadcrumb path back to its provider-specific location.
 *
 * `homeDirectory` (the native home-directory path, when known - see `getHomeDirectory` on
 * `FileManagerClient`) lets a leading `~`/`~/...` expand the same way a shell would, for the
 * local provider only: other providers (e.g. SFTP) may have their own, server-side `~`
 * convention keyed to a different home directory, so a bare/unexpandable `~` is passed through
 * unchanged rather than guessing.
 */
export function locationForPath(current: Location, path: string, homeDirectory?: string): Location {
  if (current.providerId === 'archive') {
    const archiveSeparator = path.indexOf('!');
    const outerPath = archiveSeparator < 0 ? path : path.slice(0, archiveSeparator);
    const outerUrl = new URL('file:///');
    outerUrl.pathname = outerPath.replaceAll('\\', '/');
    if (archiveSeparator < 0) {
      return { providerId: 'local', uri: outerUrl.toString() };
    }
    const innerPath = path.slice(archiveSeparator + 1).replace(/^\/+/, '');
    return {
      providerId: 'archive',
      uri: `archive://${outerUrl.toString().slice('file://'.length)}!/${innerPath}`,
    };
  }
  const url = new URL(current.uri);
  const canExpandTilde = current.providerId === 'local' && homeDirectory !== undefined;
  const expandedPath =
    canExpandTilde && path === '~'
      ? homeDirectory
      : canExpandTilde && path.startsWith('~/')
        ? `${homeDirectory.replace(/\/+$/, '')}/${path.slice(2)}`
        : path.startsWith('~')
          ? path
          : path.replaceAll('\\', '/');
  url.pathname = expandedPath.replaceAll('\\', '/');
  return { ...current, uri: url.toString() };
}

export function removeDiskUsageNodes(
  node: ScanDiskUsageResult['root'],
  sourceUris: ReadonlySet<string>,
): ScanDiskUsageResult['root'] | undefined {
  if (sourceUris.has(node.location.uri)) return undefined;
  let changed = false;
  const children = node.children.flatMap((child) => {
    const next = removeDiskUsageNodes(child, sourceUris);
    changed ||= next !== child;
    return next === undefined ? [] : [next];
  });
  if (!changed) return node;
  const previousLogicalBytes = node.children.reduce(
    (total, child) => total + child.logicalBytes,
    0,
  );
  const previousPhysicalBytes = node.children.reduce(
    (total, child) => total + child.physicalBytes,
    0,
  );
  const nextLogicalBytes = children.reduce((total, child) => total + child.logicalBytes, 0);
  const nextPhysicalBytes = children.reduce((total, child) => total + child.physicalBytes, 0);
  return {
    ...node,
    logicalBytes: Math.max(0, node.logicalBytes - previousLogicalBytes + nextLogicalBytes),
    physicalBytes: Math.max(0, node.physicalBytes - previousPhysicalBytes + nextPhysicalBytes),
    children,
  };
}

/**
 * A factory component so that per-instance state lives in the closure rather
 * than on a shared module-level object.
 */
export const AppShell: FactoryComponent<AppShellAttrs> = () => {
  let theme: Theme = DEFAULT_THEME;
  let currentSettings: Settings | undefined;
  let settingsUpdateQueue = Promise.resolve();
  let settingsDisclosureElement: HTMLDetailsElement | undefined;
  let settingsDialogOpen = false;
  let diagnosticsDialogOpen = false;
  let aboutDialogOpen = false;
  let diagnosticsDisclosureElement: HTMLDetailsElement | undefined;
  let workspaceDisclosureElement: HTMLDetailsElement | undefined;
  let registeredActions: readonly ActionDescriptor[] = [];
  let systemLocations: readonly SystemLocation[] = [];
  let systemLocationsError: string | undefined;
  let volumes: readonly Volume[] = [];
  let volumesError: string | undefined;
  /** Native home-directory path, for expanding a leading `~` typed into an address bar. */
  let homeDirectory: string | undefined;
  const unavailableLocations = new Set<string>();
  let plugins: readonly PluginDescriptor[] = [];
  let connections: readonly Connection[] = [];
  let connectionsManagerOpen = false;
  let shortcutsHelpOpen = false;
  let functionKeyModifiers: FunctionKeyModifiers = {};
  /** Last non-empty Quick Filter query per tab key, for the Ctrl+Shift+S "reactivate" shortcut. */
  const lastQuickFilterQueryByTabKey = new Map<string, string>();

  function favouriteActions(): readonly ActionDescriptor[] {
    const favourites = currentSettings?.favouriteLocations ?? [];
    return [
      {
        id: 'core.favourites',
        title: t('action', 'openFavourites'),
        description: t('action', 'showSavedLocations'),
        category: 'navigation',
        defaultShortcuts: [{ key: 'h', ctrl: true, shift: true }],
        contextRequirements: {},
        source: { kind: 'core' },
      },
      ...favourites.map((favourite, index) => ({
        id: `core.favourite.${index}`,
        title: t('action', 'openFavourite', { name: favourite.label }),
        description: favourite.location.uri,
        category: 'navigation',
        // TC-style quick-switch-to-saved-location: Ctrl/Cmd+1..9 for the first nine favourites
        // (task 0129's Alt+F1/Alt+F2 "switch panel to a different drive" row — fm has no drive
        // concept, but jumping to a saved favourite location is the closest equivalent).
        defaultShortcuts: index < 9 ? [{ key: String(index + 1), ctrl: true }] : [],
        contextRequirements: {},
        source: { kind: 'core' as const },
      })),
    ];
  }

  function localisedRegisteredActions(): readonly ActionDescriptor[] {
    return registeredActions.map((action) => ({
      ...action,
      title: actionTitle(action.id, action.title),
    }));
  }

  /** Purely frontend UI actions with no backend action-registry counterpart,
   * synthesized client-side the same way `favouriteActions` is - see `invokePaletteAction`'s
   * `client.*` branches for the dispatch side. */
  function clientOnlyActions(): readonly ActionDescriptor[] {
    return [
      {
        id: 'client.toggleDirectoryTree',
        title: t('tree', 'toggleSidebar'),
        description: t('tree', 'directoryTree'),
        category: 'navigation',
        defaultShortcuts: [{ key: 'F10', alt: true }],
        contextRequirements: {},
        source: { kind: 'core' },
      },
      {
        id: 'client.toggleOperationCentre',
        title:
          workspace?.operationCentre.visible === true
            ? t('shell', 'hideOperationCentre')
            : t('shell', 'showOperationCentre'),
        category: 'navigation',
        defaultShortcuts: [{ key: 'z', alt: true }],
        contextRequirements: {},
        source: { kind: 'core' },
      },
      {
        id: 'client.diskUsage',
        title: t('diskUsage', 'tabTitle'),
        description: t('diskUsage', 'description'),
        category: 'tools',
        defaultShortcuts: [{ key: 'l', ctrl: true, shift: true }],
        contextRequirements: {},
        source: { kind: 'core' },
      },
    ];
  }

  function actionsWithFavourites(): readonly ActionDescriptor[] {
    return [...localisedRegisteredActions(), ...clientOnlyActions(), ...favouriteActions()];
  }

  /** Every open tab across every pane, flattened for the native Window menu (task 0133). */
  function nativeMenuWindowTabs(): readonly NativeMenuTab[] {
    if (workspace === undefined) return [];
    const tabs: NativeMenuTab[] = [];
    for (const paneId of workspace.paneOrder) {
      const pane = workspace.panesById[paneId];
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

  /** Serialized form of the last spec pushed to the native menu bar - `syncNativeMenu` diffs
   * against this so an unchanged spec never re-triggers `set_native_menu` (the menu bar is
   * desktop-only chrome, not app state, so a cheap `JSON.stringify` comparison is an acceptable
   * fallback given there is no existing deep-equal utility in this codebase to reuse). */
  let lastSentNativeMenuSpecJson: string | undefined;
  let windowsNativeMenuSpec: NativeMenuSpec = { menus: [] };

  /** Set once `subscribe_native_menu_actions` has actually resolved. `syncNativeMenu` must not
   * push a spec before this: `set_native_menu` binds whatever channel is *currently* subscribed
   * as the click callback, and installs are memoized by spec content - if the very first push
   * raced ahead of the subscribe call, the backend would bind a no-op callback (nothing
   * subscribed yet) and then never rebuild the menu again to pick up the real one, since nothing
   * about the spec's content changes once subscribed. Every menu click would silently no-op. */
  let nativeMenuChannelReady = false;

  /** Pushes the full native menu bar spec to the backend whenever it might have changed. Called
   * from the view rather than threaded through every individual mutation site (registered
   * actions/settings/favourites/workspace tabs are each reassigned from several different
   * closures across this file) - safe and cheap because of the diff above, and it can never miss
   * a state change that affects the menu. */
  function syncNativeMenu(): void {
    if (runtimeKind !== 'tauri') return;
    const spec: NativeMenuSpec = buildNativeMenuSpec({
      actions: localisedRegisteredActions(),
      favouriteActions: favouriteActions(),
      tabs: nativeMenuWindowTabs(),
      canOpenNewWindow: attrsClient.openWorkspaceWindow !== undefined,
      workspaces: sortWorkspaceSummaries(workspaceSummaries),
      currentWorkspaceId: workspace?.id,
      volumes,
      connections,
      systemLocations,
      unavailableLocations,
    });
    windowsNativeMenuSpec = spec;
    if (isWindowsTauriHost() || !nativeMenuChannelReady) return;
    const serialized = JSON.stringify(spec);
    if (serialized === lastSentNativeMenuSpecJson) return;
    lastSentNativeMenuSpecJson = serialized;
    void invoke('set_native_menu', { spec }).catch(() => {
      // The native menu bar is cosmetic desktop chrome; a failed push shouldn't surface an error.
    });
  }

  const nativeMenuDispatchContext: NativeMenuDispatchContext = {
    findAction: (id) => actionsWithFavourites().find((candidate) => candidate.id === id),
    openSettingsDialog: () => {
      openSettingsDialog();
    },
    openDiagnostics: () => {
      if (diagnosticsDisclosureElement === undefined) return;
      diagnosticsDisclosureElement.open = true;
      diagnosticsDialogOpen = true;
      m.redraw();
    },
    openShortcutsHelp: () => {
      shortcutsHelpOpen = true;
      m.redraw();
    },
    activateTabByKey: (key) => {
      const separator = key.indexOf(':');
      if (separator < 0) return;
      const paneId = key.slice(0, separator);
      const tabId = key.slice(separator + 1);
      if (workspace?.panesById[paneId]?.activeTabId === tabId) {
        // The Window menu lists every open tab per pane, so clicking one that's already its
        // pane's active/displayed tab is a normal, common case: switching keyboard focus to
        // another pane without changing what it shows - there's no equivalent single click for
        // this in the tab bar (you'd click the pane's content area instead). Routing it through
        // tabController.activateTab would hit its "re-click the same tab" branch, which triggers
        // a background reload without ever updating which pane has focus and can disturb that
        // pane's existing selection. activatePane is the lightweight, no-reload "just switch
        // focus" operation this needs instead.
        void activatePane(attrsClient, paneId).catch(() => undefined);
        return;
      }
      tabController.activateTab(paneId, tabId);
    },
    activePaneId: () => activeDirectory()?.paneId,
    openNewTab: (paneId) => tabController.openTab(paneId),
    closeActiveTab: (paneId) => {
      const focusedEditorPaneId = (
        document.activeElement?.closest('.fm-file-editor') as HTMLElement | null
      )?.dataset.editorPaneId as PaneId | undefined;
      if (focusedEditorPaneId !== undefined && requestCloseEditor(focusedEditorPaneId)) return;
      const activeTabId = workspace?.panesById[paneId]?.activeTabId;
      if (activeTabId !== undefined) tabController.requestCloseTab(paneId, activeTabId);
    },
    setSort: (paneId, sort) => globalKeydownHandlerContext.setSort(paneId, sort),
    invokeAction: (action) => {
      const actionContext = actionCommandController.actionContext();
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
            : selections.get(activeTabKey(active.paneId))?.cursorEntryId;
        if (cursorEntryId !== undefined) {
          actionCommandController.invokePaletteAction(action, undefined, {
            ...actionContext,
            selectedEntryIds: [cursorEntryId],
          });
          return;
        }
      }
      actionCommandController.invokePaletteAction(action, undefined, actionContext);
    },
    openNewWorkspaceWindow: () => {
      if (workspace === undefined) return;
      // An ephemeral window forks from the named workspace its own session came from - not from
      // `workspace.id` itself, which is this window's own (possibly unsynced) ephemeral copy.
      // `forkedFrom` is `undefined` for a from-scratch default session, meaning the new window
      // seeds from the hardcoded default too, rather than forking a fork of a fork. A
      // non-ephemeral window (the main/dock window, which always loads the last-active named
      // workspace via `start_workspace`) IS itself the named workspace, so it forks from its own
      // id - otherwise "New Window" from the dock always fell back to the hardcoded default.
      void attrsClient.openWorkspaceWindow?.(
        workspace.ephemeral ? workspace.forkedFrom : workspace.id,
      );
    },
    openWorkspaceWindowById: (workspaceId) => {
      void attrsClient.openWorkspaceWindow?.(workspaceId);
    },
    resyncWorkspace: () => {
      void resyncWorkspace(attrsClient);
    },
    getVolumes: () => volumes,
    getConnections: () => connections,
    getSystemLocations: () => systemLocations,
    navigateToLocation: (location) => {
      const paneId = activeDirectory()?.paneId;
      if (paneId === undefined) return;
      void navigation.navigate(paneId, location);
    },
  };
  let installedIconThemeId: string | undefined;
  let keybindingRuntime: KeybindingRuntime = 'browser';
  let runtimeKind: RuntimeKind = 'http';
  const isWindowsTauriHost = (): boolean =>
    runtimeKind === 'tauri' && /Windows/i.test(navigator.userAgent);
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
  let loadedEntryFormatSettings: EntryFormatSettings = DEFAULT_ENTRY_FORMAT_SETTINGS;
  let currentEntryFormatSettings: EntryFormatSettings = DEFAULT_ENTRY_FORMAT_SETTINGS;
  let workspace: WorkspaceProjection | undefined;
  let workspaceError: string | undefined;
  let workspaceSummaries: readonly WorkspaceSummary[] = [];
  let workspaceActionError: string | undefined;
  let flushPendingLayoutUpdate: (() => void) | undefined;
  const dialogs = createDialogUIController();
  let findFilesOpen = false;
  let findFilesRoot: Location | undefined;
  let findFilesSearchId: string | undefined;
  const findFilesTargetPaneBySearchId = new Map<string, PaneId>();
  let findFilesError: string | undefined;
  let findFilesGeneration = 0;
  const findFilesRootsByLocationUri = new Map<string, Location>();
  /** Friendly kind and term for each search location's tab and breadcrumb labels. */
  const findFilesPresentationsByLocationUri = new Map<string, SearchPresentation>();
  /** Full params are retained separately so F3 can initialize content highlighting. */
  const findFilesParamsByLocationUri = new Map<string, FindFilesSearchParams>();
  const findFilesQueriesByLocationUri = new Map<string, SearchQuery>();
  const findFilesExecutionModesBySearchId = new Map<string, SearchExecutionMode>();
  let pendingFindFilesStarts = 0;
  const deferredSearchResultBatches = new Map<string, BackendEvent[]>();
  /** Live directory-comparison overlay state (task 0075). Marks differing entries selected in
   * both panes once a comparison completes, Total-Commander-style, rather than surfacing a
   * separate review dialog. */
  let comparisonState: ComparisonState = initialComparisonState();
  /** Live checksum-job and duplicate-scan state (spec §18, task 0077). */
  let checksumState: ChecksumState = initialChecksumState();
  let duplicateState: DuplicateState = initialDuplicateState();
  /** Registered by `WorkspaceLayoutView` (task 0089): moves DOM focus into a pane so keyboard
   * cursor navigation works immediately, e.g. right after a filename search closes its dialog. */
  let focusPane: ((paneId: PaneId) => void) | undefined;
  let focusTerminal: (() => boolean) | undefined;
  /** Registered by `DirectoryTree` (task 0139): moves DOM focus into the tree sidebar. */
  let focusDirectoryTree: (() => boolean) | undefined;
  let commandPaletteOpen = false;
  /** Composite `paneId:tabId` keys of tabs with an open terminal drawer; a terminal stays bound
   * to the tab that opened it, not the folder it happened to be showing at the time. */
  const openTerminalTabKeys = new Set<string>();
  let disposeTerminalTab: ((tabKey: string) => void) | undefined;
  /** Directory-tree sidebar (task 0139): open/closed, lazily-fetched expansion/children cache,
   * the provider root it is currently rooted at, and the active-pane location it was last
   * synced to (so `syncDirectoryTreeToActiveLocation` only does work when that location
   * actually changes, regardless of which action changed it - navigate, breadcrumb, favourite,
   * history, tab switch, or pane switch). */
  let treeSidebarOpen = false;
  let treeState: TreeChildrenState = createTreeChildrenState();
  let treeRootLocation: Location | undefined;
  let treeSyncedLocationUri: string | undefined;
  let openTerminalSupported = false;
  let platformContextMenuSupported = false;
  let nativeIconLoader: NativeIconLoader | undefined;
  let nativeIconLoaderSource: NativeIconLoader | undefined;
  let thumbnailLoader: ThumbnailLoader | undefined;
  let finderTagsLoader: FinderTagsLoader | undefined;
  let contextMenu:
    | {
        readonly paneId: PaneId;
        readonly entries: readonly EntrySummary[];
        readonly x: number;
        readonly y: number;
      }
    | undefined;
  const commandPaletteRecency = new Map<string, number>();
  /**
   * Every per-tab runtime cache below is keyed by a composite `${paneId}:${tabId}`
   * string (see {@link tabKey}) rather than by `PaneId` alone, so switching tabs
   * never bleeds one tab's directory/selection/sort/filter state into another's
   * (spec §37).
   */
  const directories = new Map<string, PaneDirectoryView>();
  const selections = new Map<string, SelectionState>();
  /** The most recently started recursive folder-size walk (task 0071, Ctrl+.) - starting a new
   * one aborts whatever the previous one was still doing, since only one result is ever shown. */
  let folderSizeCalculation: AbortController | undefined;
  /** Lister sessions are owned by tabs, so switching tabs never closes or obscures them. */
  const viewerByTab = new Map<
    string,
    {
      readonly paneId: PaneId;
      readonly tabId: TabId;
      readonly controller: FileViewerController;
      state: FileViewerState;
    }
  >();
  const diskUsageByTab = new Map<string, DiskUsageTabEntry>();
  const editorByPane = new Map<
    PaneId,
    { readonly controller: FileEditorController; state: FileEditorState }
  >();
  const sortedEntries = new Map<
    string,
    {
      readonly input: readonly EntrySummary[];
      readonly key: string;
      readonly entries: readonly EntrySummary[];
    }
  >();
  const sortRequests = new Map<string, object>();
  // Tokens guarding the async "moveCursorTo last" flow (pane-content-builder's onSelectionAction):
  // loading every page before landing the cursor takes real time, and if the user issues another
  // selection action (e.g. presses Up) before it resolves, the stale resolution must not clobber
  // whatever the newer action already set. Cleared/replaced by pane-content-builder itself.
  const cursorLoadTokens = new Map<string, object>();
  /** Whether the inline quick-filter box is shown for a pane, independent of a persisted query. */
  const quickFilterOpen = new Map<string, boolean>();
  const filteredEntries = new Map<
    string,
    {
      readonly input: readonly EntrySummary[];
      readonly query: string;
      readonly entries: readonly EntrySummary[];
    }
  >();
  /** Pending confirmation for closing a pane's only remaining tab (spec §37). */
  let closeTabConfirmation: { readonly paneId: PaneId; readonly tabId: TabId } | undefined;
  let platform: SelectionPlatform = 'unknown';
  let nativeDragOutSupported = false;
  let nativeDropInProgress = false;
  let nativeDragSourceInternal = false;
  let draggedLocations: readonly Location[] = [];
  let workspaceRequest: AbortController | undefined;
  let unsubscribeEvents: (() => void) | undefined;
  let unsubscribeNativeFileDrops: (() => void) | undefined;
  let unsubscribeConnection: (() => void) | undefined;
  let unsubscribeResynchronise: (() => void) | undefined;
  let appState: AppState | undefined;
  let operations = createOperationsState();
  let pendingConflict: OperationConflict | undefined;
  let clipboardMessage: string | undefined;
  let pendingOperationEvents: BackendEvent[] = [];
  let operationFrame: number | undefined;
  const autoDismissTimers = new Map<OperationId, ReturnType<typeof setTimeout>>();
  const operationCentreOpenTimers = new Map<OperationId, ReturnType<typeof setTimeout>>();
  const dismissedOperationIds = loadDismissedOperationIds();
  let removed = false;

  function rememberDismissedOperation(operationId: OperationId): void {
    dismissedOperationIds.add(operationId);
    persistDismissedOperationIds(dismissedOperationIds);
  }

  function clearDismissedOperation(operationId: OperationId): void {
    if (!dismissedOperationIds.delete(operationId)) return;
    persistDismissedOperationIds(dismissedOperationIds);
  }

  /** (Re)schedules an operation to auto-dismiss unless it's manually dismissed first. */
  function scheduleAutoDismiss(operationId: OperationId, delayMs: number): void {
    const existing = autoDismissTimers.get(operationId);
    if (existing !== undefined) clearTimeout(existing);
    autoDismissTimers.set(
      operationId,
      setTimeout(() => {
        autoDismissTimers.delete(operationId);
        const current = operations.byId[operationId];
        if (current !== undefined && shouldAutoDismissOperation(current)) {
          operations = dismissOperation(operations, operationId);
          m.redraw();
        }
      }, delayMs),
    );
  }

  /** Clears a pending auto-dismiss timer, e.g. once the user dismisses manually. */
  function cancelAutoDismiss(operationId: OperationId): void {
    const existing = autoDismissTimers.get(operationId);
    if (existing === undefined) return;
    clearTimeout(existing);
    autoDismissTimers.delete(operationId);
  }

  function operationIsActive(operation: Operation): boolean {
    return (
      operation.state === 'queued' ||
      operation.state === 'planning' ||
      operation.state === 'running' ||
      operation.state === 'paused' ||
      operation.state === 'waitingForConflictResolution' ||
      operation.state === 'cancelling'
    );
  }

  function scheduleOperationCentreOpen(operationId: OperationId, delayMs: number): void {
    if (operationCentreOpenTimers.has(operationId)) return;
    operationCentreOpenTimers.set(
      operationId,
      setTimeout(() => {
        operationCentreOpenTimers.delete(operationId);
        const operation = operations.byId[operationId];
        if (operation !== undefined && operationIsActive(operation)) {
          setOperationCentreVisible(true);
          m.redraw();
        }
      }, delayMs),
    );
  }

  function cancelOperationCentreOpen(operationId: OperationId): void {
    const timer = operationCentreOpenTimers.get(operationId);
    if (timer === undefined) return;
    clearTimeout(timer);
    operationCentreOpenTimers.delete(operationId);
  }

  function clearOperationSourceSelections(operation: Operation): void {
    const sourceUris = new Set(operation.sources.map((source) => source.location.uri));
    for (const [key, selection] of selections) {
      const directory = directories.get(key);
      if (directory === undefined || selection.selectedEntryIds.length === 0) continue;
      const sourceIds = new Set(
        directory.entries
          .filter((entry) => sourceUris.has(entry.location.uri))
          .map((entry) => entry.id),
      );
      const selectedEntryIds = selection.selectedEntryIds.filter((id) => !sourceIds.has(id));
      if (selectedEntryIds.length !== selection.selectedEntryIds.length) {
        selections.set(key, { ...selection, selectedEntryIds });
      }
    }
  }
  const DEFAULT_SORT: readonly SortDescriptor[] = [
    { columnId: 'core.name', direction: 'ascending' },
  ];

  /**
   * Applies theme, font size, row height, date/size format and icon theme live (task 0083,
   * extended by task 0092): shared by the initial settings load, the settings editor's live
   * preview, a successful save, and reverting on cancel.
   */
  function applyAppearance(settings: Settings): void {
    settingsController.applyAppearance(settings);
  }

  async function applyShowHiddenFilesToAllTabs(
    client: FileManagerClient,
    showHidden: boolean,
  ): Promise<void> {
    await settingsController.applyShowHiddenFilesToAllTabs(client, showHidden);
  }

  function closeSettingsDialog(): void {
    settingsController.closeSettingsDialog();
  }

  /** Opens the Settings dialog (Cmd+,/Ctrl+,) - mirrors the settings toolbar button's "open"
   * branch (`settingsDisclosureElement.open = true`) rather than toggling, so pressing the
   * shortcut again while already open is a harmless no-op instead of closing it. */
  function openSettingsDialog(): void {
    if (settingsDisclosureElement === undefined || settingsDialogOpen) return;
    settingsDisclosureElement.open = true;
    settingsDialogOpen = true;
    m.redraw();
  }

  function applyIconTheme(themeId: string): void {
    settingsController.applyIconTheme(themeId);
  }

  const systemThemeQuery: MediaQueryList | undefined =
    typeof window.matchMedia === 'function'
      ? window.matchMedia('(prefers-color-scheme: dark)')
      : undefined;
  function handleSystemThemeChange(): void {
    if (theme === 'auto') settingsController.syncTauriWindowBackground();
  }

  /** Restores real DOM keyboard focus to the active pane whenever the OS window regains it (e.g.
   * alt-tabbing back into the app). Without this, `document.activeElement` is left wherever it was
   * before the app lost focus (often nowhere useful), so the cursor row still *looks* highlighted
   * but arrow keys silently do nothing until the user clicks a row to re-establish focus manually. */
  function handleWindowFocus(): void {
    const activePaneId = workspace?.activePaneId;
    if (activePaneId === undefined) return;
    if (focusPane !== undefined) focusPane(activePaneId);
    else void activatePane(attrsClient, activePaneId);
  }

  async function loadSettings(client: FileManagerClient): Promise<void> {
    await settingsController.loadSettings(client);
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
    const pane = workspace?.panesById[paneId];
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
    const cached = sortedEntries.get(key);
    if (cached?.input === entries && cached.key === cacheKey) {
      return cached.entries;
    }
    const model = frontendSort(sort);
    if (entries.length < 10_000) {
      const sorted = sortEntries(entries, model, foldersFirst, { groupByParentPath });
      sortedEntries.set(key, { input: entries, key: cacheKey, entries: sorted });
      return sorted;
    }
    const request = {};
    sortRequests.set(key, request);
    void sortEntriesResponsive(entries, model, foldersFirst, { groupByParentPath }).then(
      (sorted) => {
        if (sortRequests.get(key) === request) {
          sortedEntries.set(key, { input: entries, key: cacheKey, entries: sorted });
          sortRequests.delete(key);
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
    const cached = filteredEntries.get(key);
    if (cached?.input === entries && cached.query === query) {
      return cached.entries;
    }
    const filtered = filterEntries(entries, query);
    filteredEntries.set(key, { input: entries, query, entries: filtered });
    return filtered;
  }

  function quickFilterQueryFor(key: string, tab: TabProjection | undefined): string {
    return appState?.quickFilterDrafts.byTabKey[key] ?? tab?.view.quickFilter?.query ?? '';
  }

  function quickFilterOpenFor(key: string, tab: TabProjection | undefined): boolean {
    return quickFilterOpen.get(key) === true || (tab?.view.quickFilter ?? null) !== null;
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
    const params = findFilesParamsByLocationUri.get(locationUri);
    if (params?.contentQuery === undefined || params.contentQuery === '') return undefined;
    // A directory listing refetched via REST (`navigation.load()`, e.g. after a subsequent
    // search batch or a plain tab switch) never carries `contentMatches` - only the live
    // `search.resultsBatch` SSE event does (`EntrySummaryDto` has no such field) - so fall back
    // to whatever that event most recently cached for this entry's location.
    const matches = entry.contentMatches ?? appState?.contentMatches.byEntryUri[entry.location.uri];
    if (matches === undefined || matches.length === 0) return undefined;
    return {
      query: params.contentQuery,
      regex: params.contentRegex,
      caseSensitive: false,
      wholeWord: true,
    };
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
    folderSizeCalculation?.abort();
    const controller = new AbortController();
    folderSizeCalculation = controller;
    void client
      .calculateFolderSize({ location: entry.location }, controller.signal)
      .then((result) => {
        if (controller.signal.aborted) return;
        const key = activeTabKey(paneId);
        const current = directories.get(key);
        if (current === undefined) return;
        directories.set(key, {
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
            error: error instanceof Error ? error.message : String(error),
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
        dialogs.openApplicationUninstallDialog({
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
            error: error instanceof Error ? error.message : String(error),
          }),
        });
      });
  }

  /** Opens the Lister-style viewer in a new tab in `paneId`. `openMetadata` shows the Alt+Space
   * info panel immediately (used when Alt+Space is pressed with no viewer already open, so the
   * shortcut works from the directory listing too, not just inside an already-open viewer). */
  function openViewer(
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
  ): void {
    const existingViewer = [...viewerByTab.entries()][0];
    if (existingViewer !== undefined) {
      const [key, viewer] = existingViewer;
      if (viewer.state.entry.location.uri === entry.location.uri) {
        closeViewer(viewer.paneId, viewer.tabId);
        return;
      }
      viewer.controller.dispose();
      viewerByTab.delete(key);
      const controller = createFileViewerController({
        client,
        entry,
        ...(workspace ? { workspaceId: workspace.id } : {}),
        ...(initialSearch ? { initialSearch } : {}),
        initialMetadataPanelOpen: openMetadata === true,
        update: (state) => {
          const current = viewerByTab.get(key);
          if (current === undefined) return;
          current.state = state;
          m.redraw();
        },
      });
      viewerByTab.set(key, {
        paneId: viewer.paneId,
        tabId: viewer.tabId,
        controller,
        state: { status: 'loading', entry },
      });
      tabController.activateTab(viewer.paneId, viewer.tabId);
      m.redraw();
      return;
    }
    const currentWorkspace = workspace;
    const pane = currentWorkspace?.panesById[paneId];
    const activeTab = pane?.tabsById[pane.activeTabId];
    if (currentWorkspace === undefined || activeTab === undefined) return;
    void dispatchWorkspaceCommand(
      client,
      {
        type: 'addTab',
        workspaceId: currentWorkspace.id,
        paneId,
        location: activeTab.location,
        expectedRevision: currentWorkspace.revision,
      },
      (next) => {
        replaceWorkspace(next);
        const tabId = next.panesById[paneId]?.activeTabId;
        if (tabId === undefined) return;
        const key = tabKey(paneId, tabId);
        const controller = createFileViewerController({
          client,
          entry,
          workspaceId: currentWorkspace.id,
          ...(initialSearch ? { initialSearch } : {}),
          initialMetadataPanelOpen: openMetadata === true,
          update: (state) => {
            const existing = viewerByTab.get(key);
            if (existing === undefined) return;
            existing.state = state;
            m.redraw();
          },
        });
        viewerByTab.set(key, {
          paneId,
          tabId,
          controller,
          state: { status: 'loading', entry },
        });
        m.redraw();
      },
    ).catch(() => undefined);
  }

  function closeViewer(paneId: PaneId, tabId?: TabId): void {
    const resolvedTabId = tabId ?? workspace?.panesById[paneId]?.activeTabId;
    if (resolvedTabId === undefined) return;
    const key = tabKey(paneId, resolvedTabId);
    const viewer = viewerByTab.get(key);
    if (viewer === undefined) return;
    viewer.controller.dispose();
    viewerByTab.delete(key);
    tabController.performCloseTab(paneId, resolvedTabId);
  }

  function openEditor(client: FileManagerClient, paneId: PaneId, entry: EntrySummary): void {
    closeViewer(paneId);
    editorByPane.get(paneId)?.controller.dispose();
    const controller = createFileEditorController({
      client,
      entry,
      update: (state) => {
        const existing = editorByPane.get(paneId);
        if (existing !== undefined) {
          existing.state = state;
          m.redraw();
        }
      },
    });
    editorByPane.set(paneId, { controller, state: { status: 'loading', entry } });
    m.redraw();
  }

  function closeEditor(paneId: PaneId): void {
    editorByPane.get(paneId)?.controller.dispose();
    editorByPane.delete(paneId);
    m.redraw();
  }

  function requestCloseEditor(paneId: PaneId): boolean {
    const editor = editorByPane.get(paneId);
    if (editor === undefined) return false;
    if (editor.controller.requestClose()) closeEditor(paneId);
    return true;
  }

  function diskUsageRootName(location: Location): string {
    const segment = location.uri.replace(/\/+$/u, '').split('/').at(-1);
    if (segment === undefined || segment === '') return '/';
    try {
      return decodeURIComponent(segment);
    } catch {
      return segment;
    }
  }

  function replaceDiskUsageNode(
    node: ScanDiskUsageResult['root'],
    replacement: ScanDiskUsageResult['root'],
  ): ScanDiskUsageResult['root'] {
    if (node.location.uri === replacement.location.uri) return replacement;
    let changed = false;
    const children = node.children.map((child) => {
      const next = replaceDiskUsageNode(child, replacement);
      changed ||= next !== child;
      return next;
    });
    if (!changed) return node;
    const previousLogicalBytes = node.children.reduce(
      (total, child) => total + child.logicalBytes,
      0,
    );
    const previousPhysicalBytes = node.children.reduce(
      (total, child) => total + child.physicalBytes,
      0,
    );
    const nextLogicalBytes = children.reduce((total, child) => total + child.logicalBytes, 0);
    const nextPhysicalBytes = children.reduce((total, child) => total + child.physicalBytes, 0);
    return {
      ...node,
      logicalBytes: Math.max(0, node.logicalBytes - previousLogicalBytes + nextLogicalBytes),
      physicalBytes: Math.max(0, node.physicalBytes - previousPhysicalBytes + nextPhysicalBytes),
      children,
    };
  }

  function applyDiskUsageProgress(
    scanId: string,
    result: ScanDiskUsageResult,
    isComplete: boolean,
  ): void {
    const entry = [...diskUsageByTab.values()].find((candidate) => candidate.scanId === scanId);
    if (entry === undefined || entry.progressComplete) return;
    const currentResult = entry.state.type === 'loaded' ? entry.state.result : undefined;
    const baseResult = entry.expansionBaseResult ?? currentResult;
    const nextResult =
      entry.expansionLocation === undefined || baseResult === undefined
        ? result
        : {
            root: replaceDiskUsageNode(baseResult.root, result.root),
            unreadableEntries: baseResult.unreadableEntries,
            unreadable: baseResult.unreadable ?? [],
            scannedEntries: result.scannedEntries ?? 0,
          };
    entry.state = {
      type: 'loaded',
      result: nextResult,
      scanning: !isComplete,
      finalizing: false,
    };
    if (isComplete) {
      entry.progressComplete = true;
      entry.expansionLocation = undefined;
      entry.expansionBaseResult = undefined;
    }
  }

  function applyDiskUsageFailure(scanId: string, message: string): void {
    const entry = [...diskUsageByTab.values()].find((candidate) => candidate.scanId === scanId);
    if (entry === undefined || entry.progressComplete) return;
    entry.progressComplete = true;
    entry.state =
      entry.state.type === 'loaded'
        ? { ...entry.state, scanning: false, error: message }
        : { type: 'error', message };
  }

  function applyDiskUsageFinalizing(scanId: string, scannedEntries: number): void {
    const entry = [...diskUsageByTab.values()].find((candidate) => candidate.scanId === scanId);
    if (entry === undefined || entry.progressComplete || entry.state.type !== 'loaded') return;
    entry.state = {
      ...entry.state,
      scanning: true,
      finalizing: true,
      result: { ...entry.state.result, scannedEntries },
    };
  }

  function startDiskUsageScan(
    paneId: PaneId,
    tabId: TabId,
    location: Location,
    expansionLocation?: Location,
  ): void {
    const key = tabKey(paneId, tabId);
    const existing = diskUsageByTab.get(key);
    existing?.abort.abort();
    const currentWorkspace = workspace;
    if (currentWorkspace === undefined) return;
    const abort = new AbortController();
    const scanId = crypto.randomUUID();
    const entry: DiskUsageTabEntry =
      existing === undefined
        ? {
            key,
            paneId,
            tabId,
            location,
            abort,
            scanId,
            progressComplete: false,
            expansionLocation,
            expansionBaseResult: undefined,
            state: {
              type: 'loading',
              rootName: diskUsageRootName(location),
            },
          }
        : {
            ...existing,
            abort,
            scanId,
            progressComplete: false,
            expansionLocation,
            expansionBaseResult:
              expansionLocation !== undefined && existing.state.type === 'loaded'
                ? existing.state.result
                : undefined,
            state:
              expansionLocation === undefined
                ? { type: 'loading', rootName: diskUsageRootName(location) }
                : existing.state.type === 'loaded'
                  ? { ...existing.state, scanning: true }
                  : existing.state,
          };
    diskUsageByTab.set(key, entry);
    m.redraw();
    void attrsClient
      .scanDiskUsage(
        {
          workspaceId: currentWorkspace.id,
          scanId,
          location,
          expandRoot: expansionLocation !== undefined,
        },
        abort.signal,
      )
      .then(() => undefined)
      .catch((error: unknown) => {
        const current = diskUsageByTab.get(entry.key);
        if (abort.signal.aborted || current?.scanId !== scanId || current.progressComplete) return;
        const message = workspaceErrorMessage(error, t('diskUsage', 'scanFailed'));
        current.progressComplete = true;
        current.state =
          current.state.type === 'loaded'
            ? { ...current.state, scanning: false, error: message }
            : { type: 'error', message };
        m.redraw();
      });
  }

  function expandDiskUsageFolder(key: string, location: Location): void {
    const entry = diskUsageByTab.get(key);
    if (entry === undefined) return;
    startDiskUsageScan(entry.paneId, entry.tabId, location, location);
  }

  function stopDiskUsage(key: string): void {
    const entry = diskUsageByTab.get(key);
    if (entry === undefined || entry.progressComplete) return;
    entry.abort.abort();
    entry.progressComplete = true;
    if (entry.state.type === 'loaded') {
      entry.state = { ...entry.state, scanning: false };
    } else {
      entry.state = { type: 'cancelled', rootName: diskUsageRootName(entry.location) };
    }
    void attrsClient.cancelDiskUsage(entry.scanId).catch((error: unknown) => {
      console.warn('Failed to cancel disk-usage scan', error);
    });
    m.redraw();
  }

  function openDiskUsage(): void {
    const active = activeDirectory();
    const currentWorkspace = workspace;
    const pane = active === undefined ? undefined : currentWorkspace?.panesById[active.paneId];
    if (active === undefined || currentWorkspace === undefined || pane === undefined) return;
    void dispatchWorkspaceCommand(
      attrsClient,
      {
        type: 'addTransientTab',
        workspaceId: currentWorkspace.id,
        paneId: active.paneId,
        location: active.location,
        expectedRevision: currentWorkspace.revision,
      },
      (next) => {
        workspace = next;
        const tabId = next.panesById[active.paneId]?.activeTabId;
        if (tabId !== undefined) startDiskUsageScan(active.paneId, tabId, active.location);
      },
    ).catch((error: unknown) => {
      toast({ html: workspaceErrorMessage(error, t('diskUsage', 'scanFailed')) });
    });
  }

  function openDiskUsageFolder(paneId: PaneId, location: Location): void {
    const oppositePaneId = workspace?.paneOrder.find((candidate) => candidate !== paneId);
    if (oppositePaneId !== undefined) void navigation.navigate(oppositePaneId, location);
  }

  let navigation: NavigationController;

  /** Clears every per-tab runtime cache for a closed tab, cancelling its in-flight request. */
  function clearTabState(paneId: PaneId, tabId: TabId): void {
    const key = tabKey(paneId, tabId);
    viewerByTab.get(key)?.controller.dispose();
    viewerByTab.delete(key);
    const diskUsage = diskUsageByTab.get(key);
    if (diskUsage !== undefined && !diskUsage.progressComplete) {
      diskUsage.abort.abort();
      void attrsClient.cancelDiskUsage(diskUsage.scanId).catch((error: unknown) => {
        console.warn('Failed to cancel disk-usage scan while closing its tab', error);
      });
    }
    diskUsageByTab.delete(key);
    navigation.abort(paneId, tabId);
    directories.delete(key);
    selections.delete(key);
    sortedEntries.delete(key);
    sortRequests.delete(key);
    if (appState !== undefined)
      appState = applyAppPatches(appState, deleteQuickFilterDraftPatch(key));
    quickFilterOpen.delete(key);
    filteredEntries.delete(key);
    if (openTerminalTabKeys.delete(key)) disposeTerminalTab?.(key);
  }

  /** Preserves tab-owned UI state when the authoritative tab moves to another pane. */
  function moveTabState(sourcePaneId: PaneId, targetPaneId: PaneId, tabId: TabId): void {
    if (sourcePaneId === targetPaneId) return;
    const sourceKey = tabKey(sourcePaneId, tabId);
    const targetKey = tabKey(targetPaneId, tabId);
    const rekey = <T>(values: Map<string, T>): void => {
      const value = values.get(sourceKey);
      if (value === undefined) return;
      values.delete(sourceKey);
      values.set(targetKey, value);
    };
    rekey(directories);
    rekey(selections);
    rekey(sortedEntries);
    rekey(sortRequests);
    rekey(cursorLoadTokens);
    rekey(quickFilterOpen);
    rekey(filteredEntries);
    const viewer = viewerByTab.get(sourceKey);
    if (viewer !== undefined) {
      viewerByTab.delete(sourceKey);
      viewerByTab.set(targetKey, { ...viewer, paneId: targetPaneId });
    }
    const diskUsage = diskUsageByTab.get(sourceKey);
    if (diskUsage !== undefined) {
      diskUsageByTab.delete(sourceKey);
      diskUsage.key = targetKey;
      diskUsage.paneId = targetPaneId;
      diskUsageByTab.set(targetKey, diskUsage);
    }
    navigation.moveTab(sourcePaneId, targetPaneId, tabId);
    const quickFilterDraft = appState?.quickFilterDrafts.byTabKey[sourceKey];
    if (appState !== undefined && quickFilterDraft !== undefined) {
      appState = applyAppPatches(
        appState,
        deleteQuickFilterDraftPatch(sourceKey),
        setQuickFilterDraftPatch(targetKey, quickFilterDraft),
      );
    }
    if (openTerminalTabKeys.delete(sourceKey)) openTerminalTabKeys.add(targetKey);
  }

  /** Releases every per-tab cache belonging to a workspace being switched away from. */
  function releaseWorkspaceTabState(outgoing: WorkspaceProjection): void {
    for (const paneId of outgoing.paneOrder) {
      for (const tabId of outgoing.panesById[paneId]?.tabOrder ?? []) {
        clearTabState(paneId, tabId);
      }
      editorByPane.get(paneId)?.controller.dispose();
      editorByPane.delete(paneId);
    }
  }

  /** Loads every pane's active tab, the currently active pane first (task 0084). */
  function loadPanesActiveFirst(loaded: WorkspaceProjection): void {
    void navigation.load(loaded.activePaneId);
    for (const paneId of loaded.paneOrder) {
      if (paneId !== loaded.activePaneId) void navigation.load(paneId);
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
    await workspaceController.switchWorkspace(workspaceId);
  }

  function refetchAffectedPanes(
    paneId?: PaneId,
    options?: { readonly background?: boolean },
  ): void {
    if (workspace === undefined) return;
    const background = options?.background ?? true;
    for (const candidate of workspace.paneOrder) {
      const activeTab =
        workspace.panesById[candidate]?.tabsById[workspace.panesById[candidate]?.activeTabId ?? ''];
      if (activeTab?.location.uri.startsWith('search://')) continue;
      // Background refreshes are used for opportunistic reloads (e.g. deltas/watch events),
      // while some callers (operation completion) request a foreground reload to guarantee
      // authoritative source/destination listings after mutating actions.
      if (paneId === undefined || candidate === paneId) {
        void navigation.load(candidate, background ? { background: true } : undefined);
      }
    }
  }

  function removeOperationSourcesFromSearchResults(operation: Operation): void {
    if (workspace === undefined || operation.sources.length === 0) return;
    const sourceUris = new Set(operation.sources.map((source) => source.location.uri));
    for (const pane of Object.values(workspace.panesById)) {
      for (const tabId of pane.tabOrder) {
        const tab = pane.tabsById[tabId];
        if (tab === undefined || !tab.location.uri.startsWith('search://')) continue;
        const key = tabKey(pane.id, tabId);
        const current = directories.get(key);
        if (current === undefined) continue;
        const removed = current.entries.filter((entry) => sourceUris.has(entry.location.uri));
        if (removed.length === 0) continue;
        directories.set(key, {
          ...current,
          entries: current.entries.filter((entry) => !sourceUris.has(entry.location.uri)),
        });
        const selection = selections.get(key);
        if (selection !== undefined) {
          selections.set(
            key,
            reduceSelection(
              selection,
              { type: 'prune', removedEntryIds: removed.map((entry) => entry.id) },
              current.entries.map((entry) => entry.id),
            ),
          );
        }
      }
    }
  }

  function removeOperationSourcesFromDiskUsage(operation: Operation): void {
    if (operation.sources.length === 0) return;
    const sourceUris = new Set(operation.sources.map((source) => source.location.uri));
    for (const entry of diskUsageByTab.values()) {
      if (entry.state.type !== 'loaded') continue;
      const root = removeDiskUsageNodes(entry.state.result.root, sourceUris);
      if (root === entry.state.result.root) continue;
      if (!entry.progressComplete) {
        entry.abort.abort();
        entry.progressComplete = true;
        void attrsClient.cancelDiskUsage(entry.scanId).catch((error: unknown) => {
          console.warn('Failed to cancel stale disk-usage scan', error);
        });
      }
      entry.state =
        root === undefined
          ? { type: 'error', message: t('diskUsage', 'rootRemoved') }
          : {
              ...entry.state,
              scanning: false,
              result: { ...entry.state.result, root },
            };
    }
  }

  function applyDelta(paneId: PaneId, delta: DirectoryDelta): void {
    const key = activeTabKey(paneId);
    const current = directories.get(key);
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
    if (delta.type === 'reset') {
      const nextEntries = delta.snapshot.entries;
      directories.set(
        key,
        respectSystemLocationReadOnly(
          {
            state: delta.snapshot.loadingState,
            entries: delta.snapshot.entries,
            location: delta.snapshot.location,
            writable: delta.snapshot.writable,
            requestId: delta.snapshot.requestId,
            revision,
            hasMore: delta.snapshot.hasMore,
            ...(delta.snapshot.continuationToken === undefined
              ? {}
              : { continuationToken: delta.snapshot.continuationToken }),
          },
          systemLocations,
        ),
      );
      reconcileSelectionAfterEntryChange(
        paneId,
        workspace?.panesById[paneId]?.activeTabId,
        current.entries,
        nextEntries,
      );
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
    directories.set(key, { ...current, revision, entries: nextEntries });
    reconcileSelectionAfterEntryChange(
      paneId,
      workspace?.panesById[paneId]?.activeTabId,
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
    const pendingCreatedLocation = dialogs.getState().pendingCreatedLocation;
    const created =
      pendingCreatedLocation === undefined
        ? undefined
        : nextEntries.find((entry) => entry.location.uri === pendingCreatedLocation);
    if (created !== undefined) {
      selections.set(
        key,
        reduceSelection(emptySelection, { type: 'selectOnly', entryId: created.id }, [created.id]),
      );
      dialogs.setPendingCreatedLocation(undefined);
      return;
    }

    const selection = selections.get(key);
    if (selection === undefined) return;
    const nextIds = new Set(nextEntries.map((entry) => entry.id));
    const removedEntryIds = previousEntries
      .filter((entry) => !nextIds.has(entry.id))
      .map((entry) => entry.id);
    if (removedEntryIds.length === 0) return;

    const tab = workspace?.panesById[paneId]?.tabsById[tabId];
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
    selections.set(
      key,
      reduceSelection(
        selection,
        { type: 'prune', removedEntryIds },
        previousVisibleEntries.map((entry) => entry.id),
      ),
    );
  }

  function activeDirectory(): { paneId: PaneId; location: Location } | undefined {
    const paneId = workspace?.activePaneId;
    const location =
      paneId === undefined ? undefined : directories.get(activeTabKey(paneId))?.location;
    return paneId === undefined || location === undefined ? undefined : { paneId, location };
  }

  /** The composite `paneId:tabId` key of the active pane's active tab, for terminal binding. */
  function activeTerminalTabKey(): string | undefined {
    const active = activeDirectory();
    return active === undefined ? undefined : activeTabKey(active.paneId);
  }

  /** Fetches and caches a tree node's children (directory-tree sidebar, task 0139), unless
   * already cached or already in flight. */
  function ensureTreeChildrenLoaded(location: Location): void {
    if (location.uri in treeState.childrenByUri || treeState.loadingUris.has(location.uri)) return;
    treeState = withLoading(treeState, location.uri, true);
    attrsClient
      .listDirectoryChildren(location, false)
      .then((children) => {
        treeState = withChildren(treeState, location.uri, children);
        m.redraw();
      })
      .catch((error: unknown) => {
        treeState = withError(
          treeState,
          location.uri,
          error instanceof Error ? error.message : t('tree', 'unableToLoad'),
        );
        m.redraw();
      });
  }

  /** Expands (fetching children if not cached) or collapses a tree node. */
  function toggleTreeNode(location: Location): void {
    if (treeState.expanded.has(location.uri)) {
      treeState = withExpanded(treeState, location.uri, false);
      return;
    }
    if (location.uri in treeState.childrenByUri) {
      treeState = withExpanded(treeState, location.uri, true);
      return;
    }
    ensureTreeChildrenLoaded(location);
  }

  /** Keeps the directory-tree sidebar's expanded/highlighted path in sync with the active
   * pane's current location, in both directions: called on every render (cheap - a single
   * string comparison when nothing changed), so it catches every way the active location can
   * change - `navigate()`, breadcrumbs, favourites, history, a tab switch, or a pane switch -
   * without needing a bespoke hook into each one. */
  function syncDirectoryTreeToActiveLocation(): void {
    const active = activeDirectory();
    if (active === undefined || active.location.uri === treeSyncedLocationUri) return;
    treeSyncedLocationUri = active.location.uri;
    const root = rootLocationFor(active.location);
    if (treeRootLocation === undefined || treeRootLocation.uri !== root.uri) {
      treeRootLocation = root;
      treeState = createTreeChildrenState();
    }
    // `ancestorChain` excludes `root` itself (a direct child of root yields an empty chain), but
    // the root row still must be expanded for that child to appear at all - so expand+load it
    // explicitly whenever the active location is anywhere below it.
    if (active.location.uri !== root.uri) {
      treeState = withExpanded(treeState, root.uri, true);
      ensureTreeChildrenLoaded(root);
    }
    for (const ancestor of ancestorChain(root, active.location)) {
      treeState = withExpanded(treeState, ancestor.uri, true);
      ensureTreeChildrenLoaded(ancestor);
    }
  }

  /** Opens/closes the directory-tree sidebar (Alt+F10, and the command palette's "Toggle
   * Directory Tree" entry, task 0139), moving DOM focus into it when it opens so arrow-key
   * navigation works immediately without an extra click. */
  function toggleDirectoryTree(): void {
    treeSidebarOpen = !treeSidebarOpen;
    if (treeSidebarOpen) {
      syncDirectoryTreeToActiveLocation();
      requestAnimationFrame(() => focusDirectoryTree?.());
    }
  }

  function setOperationCentreVisible(visible: boolean): void {
    const previous = workspace;
    if (previous === undefined || previous.operationCentre.visible === visible) return;
    const preferences = {
      ...previous.operationCentre,
      visible,
    };
    replaceWorkspace({ ...previous, operationCentre: preferences });
    void dispatchWorkspaceCommand(
      attrsClient,
      {
        type: 'updateOperationCentre',
        workspaceId: previous.id,
        preferences,
        expectedRevision: previous.revision,
      },
      replaceWorkspace,
    ).catch((error: unknown) => {
      if (workspace?.revision === previous.revision) replaceWorkspace(previous);
      toast({
        html: workspaceErrorMessage(error, t('shell', 'operationCentreUpdateFailed')),
      });
    });
  }

  function toggleOperationCentre(): void {
    setOperationCentreVisible(workspace?.operationCentre.visible !== true);
  }

  function showAllOperations(): void {
    void attrsClient
      .listOperations()
      .then((listed) => {
        dismissedOperationIds.clear();
        persistDismissedOperationIds(dismissedOperationIds);
        operations = mergeOperationHistory(operations, listed);
        m.redraw();
      })
      .catch((error: unknown) => {
        toast({
          html: workspaceErrorMessage(error, t('operation', 'historyLoadFailed')),
        });
      });
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

  function clipboard() {
    return appState?.clipboard ?? emptyClipboard;
  }

  function replaceClipboard(next = emptyClipboard): void {
    if (appState !== undefined) {
      appState = applyAppPatches(appState, clipboardPatch(next));
    }
  }

  function selectedLocations(): readonly Location[] {
    const active = activeDirectory();
    const directory =
      active === undefined ? undefined : directories.get(activeTabKey(active.paneId));
    const selection =
      active === undefined ? undefined : selections.get(activeTabKey(active.paneId));
    return getSelectedEntriesOrCursor(selection, directory?.entries ?? []).map(
      (entry) => entry.location,
    );
  }

  /**
   * Clicking a footer function-key hint re-triggers the exact same keydown
   * path a real key press would (pane.ts's local handler, then this file's
   * global keydown handler), instead of duplicating each action's dispatch
   * logic here.
   */
  function invokeFunctionKeyShortcut(shortcut: string): void {
    const parts = shortcut.split('+');
    const key = parts.pop();
    if (key === undefined) return;
    const primary = parts.includes('CTRL');
    const paneElement = document.querySelector<HTMLElement>('[data-active="true"] > .fm-pane');
    paneElement?.dispatchEvent(
      new KeyboardEvent('keydown', {
        key,
        ctrlKey: primary && platform !== 'macos',
        metaKey: primary && platform === 'macos',
        altKey: parts.includes('ALT'),
        shiftKey: parts.includes('SHIFT'),
        bubbles: true,
      }),
    );
  }

  function updateFunctionKeyModifiers(event: KeyboardEvent): void {
    const next: FunctionKeyModifiers = {
      primary: hasPrimaryModifier(event, platform),
      alt: event.altKey,
      shift: event.shiftKey,
    };
    if (
      next.primary === functionKeyModifiers.primary &&
      next.alt === functionKeyModifiers.alt &&
      next.shift === functionKeyModifiers.shift
    )
      return;
    functionKeyModifiers = next;
    m.redraw();
  }

  function resetFunctionKeyModifiers(): void {
    functionKeyModifiers = {};
    m.redraw();
  }

  function functionKeyTitle(binding: {
    readonly shortcut: string;
    readonly title: string;
  }): string {
    if (binding.shortcut === 'ALT+F3') return t('action', 'openExternally');
    if (binding.shortcut === 'ALT+SHIFT+F4') return t('action', 'externalEdit');
    return binding.title;
  }

  /** The pane currently showing an open F3 viewer, if any - mirrors `globalKeydownHandlerContext
   * .getViewer`'s per-pane lookup, just scanning every pane instead of one. There is only ever one
   * viewer open app-wide (`openViewer` reuses/replaces the existing one). */
  function openViewerPaneId(): PaneId | undefined {
    if (workspace === undefined) return undefined;
    for (const paneId of workspace.paneOrder) {
      const tabId = workspace.panesById[paneId]?.activeTabId;
      if (tabId !== undefined && viewerByTab.get(tabKey(paneId, tabId)) !== undefined) {
        return paneId;
      }
    }
    return undefined;
  }

  /** Moves keyboard focus into `paneId`'s open F3 viewer - see `GlobalKeydownContext.focusViewer`. */
  function focusViewer(paneId: PaneId): void {
    const root = document.querySelector<HTMLElement>(`[data-pane-id="${paneId}"]`);
    const searchInput = root?.querySelector<HTMLInputElement>('.fm-file-viewer-search-input');
    const section = root?.querySelector<HTMLElement>('.fm-pane-viewer');
    (searchInput ?? section)?.focus();
  }

  /** One `scrollViewer('line', ...)` step, in CSS pixels - roughly a text line or a comfortable
   * image-pan increment. */
  const VIEWER_SCROLL_LINE_STEP_PX = 48;
  /** Fraction of the viewer body's own size scrolled per `scrollViewer('page', ...)` step - kept
   * just under a full screenful (rather than exactly 1) so a little of the previous page stays
   * visible as a reading anchor, matching most text editors' Page Up/Down. */
  const VIEWER_SCROLL_PAGE_FRACTION = 0.9;

  /** Scrolls/pages `paneId`'s open F3 viewer's scrollable body - see
   * `GlobalKeydownContext.scrollViewer`. */
  function scrollViewer(
    paneId: PaneId,
    dx: -1 | 0 | 1,
    dy: -1 | 0 | 1,
    unit: 'line' | 'page',
  ): void {
    const container = document.querySelector<HTMLElement>(
      `[data-pane-id="${paneId}"] .fm-file-viewer-body`,
    );
    if (container === null) return;
    const stepX =
      unit === 'page'
        ? container.clientWidth * VIEWER_SCROLL_PAGE_FRACTION
        : VIEWER_SCROLL_LINE_STEP_PX;
    const stepY =
      unit === 'page'
        ? container.clientHeight * VIEWER_SCROLL_PAGE_FRACTION
        : VIEWER_SCROLL_LINE_STEP_PX;
    container.scrollBy({ left: dx * stepX, top: dy * stepY });
  }

  const backendEventContext: BackendEventContext = {
    getWorkspaceId: () => workspace?.id,
    getWorkspaceRevision: () => workspace?.revision,
    replaceWorkspace: applyRemoteWorkspaceSnapshot,
    refreshWorkspaceSummaries: () => workspaceController.refreshWorkspaceSummaries(),
    setWorkspaceSummaries: (summaries) => {
      workspaceSummaries = summaries;
    },
    setWorkspaceActionError: (message) => {
      workspaceActionError = message;
    },
    recoverActiveWorkspace: (summaries) => workspaceController.recoverActiveWorkspace(summaries),
    listWorkspaces: () => attrsClient.listWorkspaces(),
    getWorkspace: (id) => attrsClient.getWorkspace(id),
    setPendingConflict: (conflict) => {
      pendingConflict = conflict;
    },
    getPendingOperationEvents: () => pendingOperationEvents,
    pushPendingOperationEvent: (event) => {
      pendingOperationEvents.push(event);
    },
    clearPendingOperationEvents: () => {
      const events = pendingOperationEvents;
      pendingOperationEvents = [];
      return events;
    },
    getOperationFrame: () => operationFrame,
    setOperationFrame: (frame) => {
      operationFrame = frame;
    },
    getOperations: () => operations,
    setOperations: (next) => {
      operations = next;
    },
    getDismissedOperationIds: () => dismissedOperationIds,
    clearDismissedOperation,
    scheduleAutoDismiss,
    scheduleOperationCentreOpen,
    cancelOperationCentreOpen,
    clearOperationSourceSelections,
    removeOperationSourcesFromSearchResults,
    removeOperationSourcesFromDiskUsage,
    getActiveDirectoryRevision: (paneId) => directories.get(activeTabKey(paneId))?.revision,
    applyDelta,
    refetchAffectedPanes,
    getPlugins: () => plugins,
    setPlugins: (next) => {
      plugins = next;
    },
    listPlugins: () => attrsClient.listPlugins(),
    getCurrentIconThemeSetting: () => currentSettings?.iconTheme,
    applyIconTheme,
    getConnections: () => connections,
    setConnections: (next) => {
      connections = next;
    },
    getConnection: (id) => attrsClient.getConnection(id),
    getComparisonState: () => comparisonState,
    setComparisonState: (next) => {
      comparisonState = next;
    },
    getChecksumState: () => checksumState,
    setChecksumState: (next) => {
      checksumState = next;
    },
    getDuplicateState: () => duplicateState,
    setDuplicateState: (next) => {
      duplicateState = next;
    },
    markComparisonDifferences: (state) => {
      for (const paneId of [state.leftPaneId, state.rightPaneId]) {
        if (paneId === undefined) continue;
        const key = activeTabKey(paneId);
        const directory = directories.get(key);
        if (directory === undefined) continue;
        const matchingIds = differingEntryIds(state, paneId, directory.entries);
        if (matchingIds.length === 0) continue;
        const orderedEntryIds = directory.entries.map((entry) => entry.id);
        selections.set(
          key,
          reduceSelection(
            selections.get(key) ?? emptySelection,
            { type: 'restore', entryIds: matchingIds },
            orderedEntryIds,
          ),
        );
      }
    },
    getFindFilesSearchId: () => findFilesSearchId,
    getFindFilesTargetPane: (searchId) => findFilesTargetPaneBySearchId.get(searchId),
    clearFindFilesTargetPane: (searchId) => {
      findFilesTargetPaneBySearchId.delete(searchId);
    },
    hasPendingFindFilesStart: () => pendingFindFilesStarts > 0,
    deferSearchResultBatch: (event) => {
      if (event.payload.type !== 'search.resultsBatch') return;
      const pending = deferredSearchResultBatches.get(event.payload.searchId) ?? [];
      pending.push(event);
      deferredSearchResultBatches.set(event.payload.searchId, pending);
    },
    setSearchExecutionMode: (uri, executionMode) => {
      const searchId = uri.slice('search://local/'.length);
      findFilesExecutionModesBySearchId.set(searchId, executionMode);
      const presentation = findFilesPresentationsByLocationUri.get(uri);
      if (presentation !== undefined) {
        findFilesPresentationsByLocationUri.set(uri, { ...presentation, executionMode });
      }
    },
    cacheContentMatches: (uri, matches) => {
      if (appState !== undefined)
        appState = applyAppPatches(appState, cacheContentMatchesPatch(uri, matches));
    },
    findPanesWithUri: (uri) =>
      workspace === undefined
        ? []
        : (
            Object.entries(workspace.panesById) as Array<
              [PaneId, WorkspaceProjection['panesById'][PaneId]]
            >
          )
            .filter(([, pane]) => pane.tabsById[pane.activeTabId]?.location.uri === uri)
            .map(([paneId]) => paneId),
    revealSearchResults: async (searchId) => {
      const searchUri = `search://local/${searchId}`;
      const paneId = findFilesTargetPaneBySearchId.get(searchId);
      if (paneId === undefined) return [];
      await navigation.navigate(paneId, { providerId: 'search', uri: searchUri });
      focusPane?.(paneId);
      m.redraw();
      return [paneId];
    },
    loadPane: (paneId, options) => navigation.load(paneId, options),
    reportSearchCompletion: (paneId, searchId) => {
      const searchUri = `search://local/${searchId}`;
      const active = activeDirectory();
      // Only react while the pane is still showing this exact search: the user may have already
      // navigated elsewhere, or started a newer search, by the time results finish streaming in.
      if (active === undefined || active.paneId !== paneId || active.location.uri !== searchUri) {
        return;
      }
      const entries = directories.get(activeTabKey(paneId))?.entries ?? [];
      if (entries.length > 0) return;
      toast({ html: t('search', 'noResultsToast') });
      const root = findFilesRootsByLocationUri.get(searchUri);
      if (root !== undefined) void navigation.navigate(paneId, root);
    },
    reportSearchWithoutResults: () => {
      toast({ html: t('search', 'noResultsToast') });
    },
    applyDiskUsageProgress,
    applyDiskUsageFinalizing,
    applyDiskUsageFailure,
    redraw: () => m.redraw(),
  };
  const handleBackendEvent = createBackendEventHandler(backendEventContext);

  let attrsClient: FileManagerClient;
  /** `dispose()` of the `buildControllers` registry set up in `oninit` (controller-registry.ts). */
  let disposeShellControllers: (() => void) | undefined;
  let opsController: OperationsController;
  let workspaceController: WorkspaceController;
  let tabController: TabController;
  let settingsController: SettingsController;
  let findFilesController: FindFilesController;
  let comparisonController: ComparisonController;
  let checksumController: ChecksumController;
  let actionCommandController: ActionCommandController;

  const workspaceControllerContext: WorkspaceControllerContext = {
    getWorkspace: () => workspace,
    setWorkspace: (ws) => {
      workspace = ws;
    },
    getWorkspaceError: () => workspaceError,
    setWorkspaceError: (msg) => {
      workspaceError = msg;
    },
    getWorkspaceSummaries: () => workspaceSummaries,
    setWorkspaceSummaries: (summaries) => {
      workspaceSummaries = summaries;
    },
    getWorkspaceActionError: () => workspaceActionError,
    setWorkspaceActionError: (msg) => {
      workspaceActionError = msg;
    },
    getWorkspaceRequest: () => workspaceRequest,
    setWorkspaceRequest: (ac) => {
      workspaceRequest = ac;
    },
    getPlatform: () => platform,
    setPlatform: (p) => {
      platform = p;
    },
    getNativeDragOutSupported: () => nativeDragOutSupported,
    setNativeDragOutSupported: (v) => {
      nativeDragOutSupported = v;
    },
    getUnsubscribeNativeFileDrops: () => unsubscribeNativeFileDrops,
    setUnsubscribeNativeFileDrops: (fn) => {
      unsubscribeNativeFileDrops = fn;
    },
    subscribeNativeFileDrops: (callback) => attrsClient.subscribeNativeFileDrops(callback),
    getDraggedLocations: () => draggedLocations,
    getNativeDragSourceInternal: () => nativeDragSourceInternal,
    setNativeDragSourceInternal: (v) => {
      nativeDragSourceInternal = v;
    },
    setOpenTerminalSupported: (v) => {
      openTerminalSupported = v;
    },
    setPlatformContextMenuSupported: (v) => {
      platformContextMenuSupported = v;
    },
    setNativeIconLoader: (loader) => {
      nativeIconLoaderSource = loader;
      nativeIconLoader = currentSettings?.iconTheme === 'native' ? loader : undefined;
    },
    setThumbnailLoader: (loader) => {
      thumbnailLoader = loader;
    },
    setFinderTagsLoader: (loader) => {
      finderTagsLoader = loader;
    },
    getSystemLocations: () => systemLocations,
    setSystemLocations: (locs) => {
      systemLocations = locs;
    },
    setSystemLocationsError: (msg) => {
      systemLocationsError = msg;
    },
    getVolumes: () => volumes,
    setVolumes: (vols) => {
      volumes = vols;
    },
    setVolumesError: (msg) => {
      volumesError = msg;
    },
    setHomeDirectory: (path) => {
      homeDirectory = path;
    },
    getConnections: () => connections,
    setConnections: (conns) => {
      connections = conns;
    },
    setDraggedLocations: (locs) => {
      draggedLocations = locs;
    },
    getNativeDropInProgress: () => nativeDropInProgress,
    setNativeDropInProgress: (v) => {
      nativeDropInProgress = v;
    },
    setClipboardMessage: (msg) => {
      clipboardMessage = msg;
    },
    getNavigation: () => navigation,
    getFlushPendingLayoutUpdate: () => flushPendingLayoutUpdate,
    redraw: () => m.redraw(),
    releaseWorkspaceTabState: (outgoing) => releaseWorkspaceTabState(outgoing),
    loadPanesActiveFirst: (ws) => loadPanesActiveFirst(ws),
    syncWorkspaceViewSettings: () => {
      if (currentSettings !== undefined) {
        void settingsController.applyShowHiddenFilesToAllTabs(
          attrsClient,
          currentSettings.showHiddenFiles,
        );
      }
    },
  };

  const tabControllerContext: TabControllerContext = {
    getWorkspace: () => workspace,
    setWorkspace: (ws) => {
      workspace = ws;
    },
    getAppState: () => appState,
    setAppState: (state) => {
      appState = state;
    },
    getNavigation: () => navigation,
    redraw: () => m.redraw(),
    applyCurrentShowHiddenSetting: (client, workspaceId, paneId, tabId, rev) =>
      settingsController.applyCurrentShowHiddenSetting(client, workspaceId, paneId, tabId, rev),
    clearTabState,
    getCloseTabConfirmation: () => closeTabConfirmation,
    setCloseTabConfirmation: (conf) => {
      closeTabConfirmation = conf;
    },
    hasCachedSnapshot: (paneId, tabId) =>
      directories.get(tabKey(paneId, tabId))?.state.type === 'loaded',
  };

  const settingsControllerContext: SettingsControllerContext = {
    setTheme: (t) => {
      theme = t;
    },
    setLoadedEntryFormatSettings: (s) => {
      loadedEntryFormatSettings = s;
    },
    getSettingsDialogOpen: () => settingsDialogOpen,
    setSettingsDialogOpen: (open) => {
      settingsDialogOpen = open;
    },
    getSettingsDisclosureElement: () => settingsDisclosureElement,
    getCurrentSettings: () => currentSettings,
    setCurrentSettings: (s) => {
      currentSettings = s;
    },
    getPlugins: () => plugins,
    getInstalledIconThemeId: () => installedIconThemeId,
    setInstalledIconThemeId: (id) => {
      installedIconThemeId = id;
    },
    setNativeIconLoaderEnabled: (enabled) => {
      nativeIconLoader = enabled ? nativeIconLoaderSource : undefined;
    },
    getRuntimeKind: () => runtimeKind,
    getWorkspace: () => workspace,
    setWorkspace: (ws) => {
      workspace = ws;
    },
    getDirectories: () => directories,
    getNavigation: () => navigation,
    getClient: () => attrsClient,
    redraw: () => m.redraw(),
  };

  const globalKeydownHandlerContext: GlobalKeydownContext = {
    getCommandPaletteOpen: () => commandPaletteOpen,
    getPlatform: () => platform,
    getKeybindingRuntime: () => keybindingRuntime,
    getCurrentSettings: () => currentSettings,
    getWorkspace: () => workspace,
    getSelections: () => selections,
    getDirectories: () => directories,
    getRegisteredActions: () => registeredActions,
    clipboard,
    getFindFilesOpen: () => findFilesOpen,
    getViewer: (paneId) => {
      const tabId = workspace?.panesById[paneId]?.activeTabId;
      return tabId === undefined ? undefined : viewerByTab.get(tabKey(paneId, tabId));
    },
    getArchiveCreateRequest: () => dialogs.getState().archiveCreateRequest,
    getCreateDirectoryOpen: () => dialogs.getState().createDirectoryOpen,
    getCreateFileOpen: () => dialogs.getState().createFileOpen,
    getAppState: () => appState,
    getLastQuickFilterQuery: (paneId) => lastQuickFilterQueryByTabKey.get(activeTabKey(paneId)),
    getShortcutsHelpOpen: () => shortcutsHelpOpen,
    setCommandPaletteOpen: (open) => {
      commandPaletteOpen = open;
    },
    setClipboardMessage: (msg) => {
      clipboardMessage = msg;
    },
    setArchiveCreateRequest: (req) => {
      if (req !== undefined) dialogs.openArchiveCreate(req);
    },
    setCreateDirectoryOpen: (open) => {
      if (open) dialogs.openCreateDirectory();
      else dialogs.cancelCreateDirectory();
    },
    setCreateFileOpen: (open) => {
      if (open) dialogs.openCreateFile();
      else dialogs.cancelCreateFile();
    },
    setAppState: (state) => {
      appState = state;
    },
    setQuickFilterOpen: (key, open) => {
      quickFilterOpen.set(key, open);
    },
    setActiveTabQuickFilter: (paneId, query) => {
      const liveWorkspace = workspace;
      const pane = liveWorkspace?.panesById[paneId];
      const tab = pane === undefined ? undefined : pane.tabsById[pane.activeTabId];
      if (liveWorkspace === undefined || tab === undefined) return;
      const key = activeTabKey(paneId);
      const previous = tab.view.quickFilter?.query ?? '';
      if (query === undefined) {
        if (previous.length > 0) lastQuickFilterQueryByTabKey.set(key, previous);
        quickFilterOpen.set(key, false);
      } else {
        quickFilterOpen.set(key, true);
      }
      if (appState !== undefined)
        appState = applyAppPatches(appState, deleteQuickFilterDraftPatch(key));
      void dispatchWorkspaceCommand(
        attrsClient,
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
          workspace = next;
        },
      ).catch(() => undefined);
    },
    setConnectionsManagerOpen: (open) => {
      connectionsManagerOpen = open;
    },
    setShortcutsHelpOpen: (open) => {
      shortcutsHelpOpen = open;
    },
    getTabController: () => tabController,
    getOpsController: () => opsController,
    getNavigation: () => navigation,
    activeDirectory,
    activeTabKey,
    actionsWithFavourites,
    openFindFiles: () => findFilesController.openFindFiles(),
    replaceClipboard,
    selectedLocations,
    invokeActionById: (actionId, parameters, context) =>
      actionCommandController.invokeActionById(actionId, parameters, context),
    openViewer: (paneId, entry, initialSearch, openMetadata) =>
      openViewer(attrsClient, paneId, entry, initialSearch, openMetadata),
    openEditor: (paneId, entry) => openEditor(attrsClient, paneId, entry),
    calculateFolderSize: (paneId, entry) => calculateFolderSize(attrsClient, paneId, entry),
    uninstallApplication: (paneId, entry) => uninstallApplication(attrsClient, paneId, entry),
    actionContext: () => actionCommandController.actionContext(),
    commandAvailabilityContext: (selectedEntries, paneId) =>
      actionCommandController.commandAvailabilityContext(selectedEntries, paneId),
    contentSearchInitialQuery,
    refetchAffectedPanes,
    platformActionParameters: (actionId, selectedEntries, directoryLocation) =>
      actionCommandController.platformActionParameters(
        actionId,
        selectedEntries,
        directoryLocation,
      ),
    activatePane: (paneId) => activatePane(attrsClient, paneId),
    focusPane: (paneId) => {
      if (focusPane !== undefined) focusPane(paneId);
      else void activatePane(attrsClient, paneId);
    },
    focusViewer,
    scrollViewer,
    toggleTerminal: () => {
      if (runtimeKind !== 'tauri') return;
      const key = activeTerminalTabKey();
      if (key === undefined) return;
      if (openTerminalTabKeys.has(key)) {
        openTerminalTabKeys.delete(key);
      } else {
        openTerminalTabKeys.add(key);
        requestAnimationFrame(() => focusTerminal?.());
      }
    },
    toggleDirectoryTree,
    toggleOperationCentre,
    redraw: () => m.redraw(),
    setSort: (paneId, sort) => {
      const liveWorkspace = workspace;
      const pane = liveWorkspace?.panesById[paneId];
      const tab = pane === undefined ? undefined : pane.tabsById[pane.activeTabId];
      if (liveWorkspace === undefined || tab === undefined) return;
      void dispatchWorkspaceCommand(
        attrsClient,
        {
          type: 'updateView',
          workspaceId: liveWorkspace.id,
          paneId,
          tabId: tab.id,
          patch: { sort: [...sort] },
          expectedRevision: liveWorkspace.revision,
        },
        (next) => {
          workspace = next;
        },
      ).catch(() => undefined);
    },
    swapPaneTabSets: (paneAId, paneBId) => {
      const liveWorkspace = workspace;
      if (liveWorkspace === undefined) return;
      const paneA = liveWorkspace.panesById[paneAId];
      const paneB = liveWorkspace.panesById[paneBId];
      if (paneA === undefined || paneB === undefined) return;
      const movedDiskUsage = new Map<string, DiskUsageTabEntry>();
      for (const tabId of paneA.tabOrder) {
        const entry = diskUsageByTab.get(tabKey(paneAId, tabId));
        if (entry !== undefined) {
          diskUsageByTab.delete(tabKey(paneAId, tabId));
          entry.key = tabKey(paneBId, tabId);
          entry.paneId = paneBId;
          movedDiskUsage.set(entry.key, entry);
        }
      }
      for (const tabId of paneB.tabOrder) {
        const entry = diskUsageByTab.get(tabKey(paneBId, tabId));
        if (entry !== undefined) {
          diskUsageByTab.delete(tabKey(paneBId, tabId));
          entry.key = tabKey(paneAId, tabId);
          entry.paneId = paneAId;
          movedDiskUsage.set(entry.key, entry);
        }
      }
      for (const [key, entry] of movedDiskUsage) diskUsageByTab.set(key, entry);
      // No backend command swaps a whole tab set atomically (task 0128 Agent Notes) - this
      // mutates the local projection directly, the same optimistic-update pattern
      // `activateTab` uses, rather than round-tripping through `dispatchWorkspaceCommand`.
      workspace = {
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
      void navigation.load(paneAId);
      void navigation.load(paneBId);
    },
    openMultiRenameForActivePane: () => {
      const active = activeDirectory();
      if (active === undefined) return;
      const key = activeTabKey(active.paneId);
      const directory = directories.get(key);
      if (directory === undefined) return;
      const selection = selections.get(key);
      const selected = getSelectedEntries(selection, directory.entries).filter(
        (entry) => !isParentEntry(entry.id),
      );
      // Total Commander's Multi Rename Tool defaults to every entry in the directory when
      // nothing is selected, rather than requiring a selection first.
      const entriesToRename =
        selected.length > 0
          ? selected
          : directory.entries.filter((entry) => !isParentEntry(entry.id));
      if (entriesToRename.length === 0) return;
      const selectedIds = new Set(entriesToRename.map((entry) => entry.id));
      dialogs.openMultiRename(
        entriesToRename,
        active.location,
        new Set(
          directory.entries
            .filter((entry) => !selectedIds.has(entry.id))
            .map((entry) => entry.name),
        ),
      );
      m.redraw();
    },
    openPropertiesForActivePane: () => {
      const active = activeDirectory();
      if (active === undefined) return;
      const key = activeTabKey(active.paneId);
      const directory = directories.get(key);
      if (directory === undefined) return;
      const selection = selections.get(key);
      const entries = getSelectedEntriesOrCursor(selection, directory.entries).filter(
        (entry) => !isParentEntry(entry.id),
      );
      if (entries.length === 0) return;
      dialogs.openProperties(entries);
      m.redraw();
    },
    quitApplication: () => {
      if (keybindingRuntime !== 'desktop') return;
      void attrsClient.quit?.();
    },
    startComparison: () => comparisonController.startComparison('sizeAndTimestamp'),
    calculateChecksums: () => checksumController.calculateChecksums(['sha256']),
    findDuplicates: () => checksumController.findDuplicates(),
    openDiskUsage,
    openSettingsDialog,
  };

  const findFilesControllerContext: FindFilesControllerContext = {
    getFindFilesOpen: () => findFilesOpen,
    setFindFilesOpen: (open) => {
      findFilesOpen = open;
    },
    getFindFilesRoot: () => findFilesRoot,
    setFindFilesRoot: (root) => {
      findFilesRoot = root;
    },
    getFindFilesSearchId: () => findFilesSearchId,
    setFindFilesSearchId: (searchId) => {
      findFilesSearchId = searchId;
      if (searchId !== undefined) {
        for (const event of deferredSearchResultBatches.get(searchId) ?? []) {
          handleBackendEvent(event);
        }
        deferredSearchResultBatches.delete(searchId);
      }
    },
    setFindFilesTargetPane: (searchId, paneId) => {
      findFilesTargetPaneBySearchId.set(searchId, paneId);
      for (const event of deferredSearchResultBatches.get(searchId) ?? []) {
        handleBackendEvent(event);
      }
      deferredSearchResultBatches.delete(searchId);
    },
    clearFindFilesTargetPane: (searchId) => {
      findFilesTargetPaneBySearchId.delete(searchId);
    },
    setFindFilesSearchStartPending: (pending) => {
      pendingFindFilesStarts = Math.max(0, pendingFindFilesStarts + (pending ? 1 : -1));
      if (pendingFindFilesStarts === 0) deferredSearchResultBatches.clear();
    },
    getFindFilesError: () => findFilesError,
    setFindFilesError: (error) => {
      findFilesError = error;
    },
    getFindFilesGeneration: () => findFilesGeneration,
    setFindFilesGeneration: (generation) => {
      findFilesGeneration = generation;
    },
    getFindFilesRootsByLocationUri: () => findFilesRootsByLocationUri,
    getFindFilesPresentationsByLocationUri: () => findFilesPresentationsByLocationUri,
    getFindFilesParamsByLocationUri: () => findFilesParamsByLocationUri,
    getFindFilesQueriesByLocationUri: () => findFilesQueriesByLocationUri,
    getSearchExecutionMode: (searchId) => findFilesExecutionModesBySearchId.get(searchId),
    getActiveDirectory: () => activeDirectory(),
    getWorkspace: () => workspace,
    getClient: () => attrsClient,
    getPaneLocationUri: (paneId) => {
      const pane = workspace?.panesById[paneId];
      return pane?.tabsById[pane.activeTabId]?.location.uri;
    },
    openTabAt: (paneId, location, historyOrigin) =>
      tabController.openTabAt(paneId, location, historyOrigin),
    reportLimitations: (message) => toast({ html: t('search', 'filterLimitations', { message }) }),
    redraw: () => m.redraw(),
  };

  const comparisonControllerContext: ComparisonControllerContext = {
    getState: () => comparisonState,
    setState: (next) => {
      comparisonState = next;
    },
    getWorkspace: () => workspace,
    getClient: () => attrsClient,
    redraw: () => m.redraw(),
  };

  const checksumControllerContext: ChecksumControllerContext = {
    getChecksumState: () => checksumState,
    setChecksumState: (next) => {
      checksumState = next;
    },
    getDuplicateState: () => duplicateState,
    setDuplicateState: (next) => {
      duplicateState = next;
    },
    getWorkspace: () => workspace,
    getClient: () => attrsClient,
    getSelectedEntries: () => {
      const active = activeDirectory();
      if (active === undefined) return [];
      const key = activeTabKey(active.paneId);
      return getSelectedEntriesOrCursor(selections.get(key), directories.get(key)?.entries ?? []);
    },
    getActiveLocation: () => activeDirectory()?.location,
    // Reuses the same operation call `core.delete` makes, so duplicate
    // deletion inherits its confirmation, conflict handling and audit trail
    // instead of introducing a second delete path (spec §35, task 0077).
    requestDelete: (locations) => {
      if (locations.length === 0) return;
      void opsController.delete(
        [...locations],
        currentSettings?.confirmPermanentDelete === false,
        false,
      );
    },
    redraw: () => m.redraw(),
  };

  const actionCommandControllerContext: ActionCommandControllerContext = {
    getCommandPaletteOpen: () => commandPaletteOpen,
    setCommandPaletteOpen: (open) => {
      commandPaletteOpen = open;
    },
    getContextMenu: () => contextMenu,
    setContextMenu: (menu) => {
      contextMenu = menu;
    },
    getCommandPaletteRecency: () => commandPaletteRecency,
    getActiveDirectory: () => activeDirectory(),
    getActiveTabKey: (paneId) => activeTabKey(paneId),
    getSelections: () => selections,
    getDirectories: () => directories,
    getCurrentSettings: () => currentSettings,
    getClient: () => attrsClient,
    getRegisteredActions: () => registeredActions,
    getWorkspace: () => workspace,
    getNavigation: () => navigation,
    getOpsController: () => opsController,
    getGetSelectedEntries: () => getSelectedEntriesOrCursor,
    getClipboard: () => clipboard(),
    replaceClipboard: (next) => replaceClipboard(next),
    toast: (options) => toast(options),
    getOpenTerminalSupported: () => openTerminalSupported,
    openCreateDirectory: (location) => dialogs.openCreateDirectory(location),
    openFinderTagsDialog: (request) => dialogs.openFinderTagsDialog(request),
    openSpotlightCommentDialog: (request) => dialogs.openSpotlightCommentDialog(request),
    setArchiveCreateRequest: (request) => dialogs.openArchiveCreate(request),
    calculateChecksums: () => checksumController.calculateChecksums(['sha256']),
    findDuplicates: () => checksumController.findDuplicates(),
    openDiskUsage,
    openPropertiesForActivePane: () => globalKeydownHandlerContext.openPropertiesForActivePane(),
    uninstallApplication: (paneId, entry) =>
      globalKeydownHandlerContext.uninstallApplication(paneId, entry),
    toggleDirectoryTree,
    toggleOperationCentre,
    redraw: () => m.redraw(),
  };

  let paneContentBuilder: (
    client: FileManagerClient,
    entryFormatSettings: EntryFormatSettings,
    paneId: PaneId,
  ) => WorkspacePaneContent;

  const paneContentBuilderContext: PaneContentContext = {
    getWorkspace: () => workspace,
    getCurrentSettings: () => currentSettings,
    getSystemLocations: () => systemLocations,
    getSystemLocationsError: () => systemLocationsError,
    getVolumes: () => volumes,
    getVolumesError: () => volumesError,
    getConnections: () => connections,
    getUnavailableLocations: () => unavailableLocations,
    getNativeIconLoader: () => nativeIconLoader,
    getThumbnailLoader: () => thumbnailLoader,
    getFinderTagsLoader: () => finderTagsLoader,
    getPlugins: () => plugins,
    getPlatform: () => platform,
    getKeybindingRuntime: () => keybindingRuntime,
    getRegisteredActions: () => registeredActions,
    getDraggedLocations: () => draggedLocations,
    getNativeDragOutSupported: () => nativeDragOutSupported,
    getNativeDropInProgress: () => nativeDropInProgress,
    getAppState: () => appState,
    clipboard,
    getDirectories: () => directories,
    getSelections: () => selections,
    getSortedEntries: () => sortedEntries,
    getSortRequests: () => sortRequests,
    getCursorLoadTokens: () => cursorLoadTokens,
    getViewerByTab: () => viewerByTab,
    getEditorByPane: () => editorByPane,
    getDiskUsageByTab: () => diskUsageByTab,
    setConnections: (conns) => {
      connections = conns;
    },
    setConnectionsManagerOpen: (open) => {
      connectionsManagerOpen = open;
    },
    setAppState: (state) => {
      appState = state;
    },
    setQuickFilterOpen: (key, open) => {
      quickFilterOpen.set(key, open);
    },
    setDraggedLocations: (locs) => {
      draggedLocations = locs;
    },
    setNativeDragSourceInternal: (v) => {
      nativeDragSourceInternal = v;
    },
    setClipboardMessage: (msg) => {
      clipboardMessage = msg;
    },
    setMultiRenameOpen: (open) => {
      if (!open) dialogs.cancelMultiRename();
    },
    setMultiRenameEntries: (entries) => {
      dialogs.getState().multiRenameEntries = entries;
    },
    setMultiRenameLocation: (location) => {
      dialogs.getState().multiRenameLocation = location;
    },
    setMultiRenameExistingNames: (names) => {
      dialogs.getState().multiRenameExistingNames = names;
    },
    tabKey,
    effectiveSort,
    frontendSort,
    sortLabel,
    entriesSortedFor,
    entriesFilteredFor,
    quickFilterQueryFor,
    quickFilterOpenFor,
    contentSearchInitialQuery,
    searchQueryForLocationUri: (locationUri) => findFilesQueriesByLocationUri.get(locationUri),
    searchFavouriteNameForLocationUri: (locationUri) => {
      const presentation = findFilesPresentationsByLocationUri.get(locationUri);
      return presentation?.label ?? presentation?.term;
    },
    workspaceErrorMessage,
    locationForPath: (current, path) => locationForPath(current, path, homeDirectory),
    activeDirectory,
    getNavigation: () => navigation,
    getWorkspaceController: () => workspaceController,
    getOpsController: () => opsController,
    openSavedSearch: (saved) => findFilesController.startSavedSearch(saved, 'currentPane'),
    openViewer: (paneId, entry, initialSearch, openMetadata) =>
      openViewer(attrsClient, paneId, entry, initialSearch, openMetadata),
    closeViewer,
    closeEditor,
    updateLocationSettings,
    invokeActionById: (actionId, parameters, context) =>
      actionCommandController.invokeActionById(actionId, parameters, context),
    openContextMenu: (paneId, entries, x, y) =>
      actionCommandController.openContextMenu(paneId, entries, x, y),
    refetchAffectedPanes,
    replaceWorkspace,
    openDiskUsageFolder,
    expandDiskUsageFolder,
    retryDiskUsage: (key) => {
      const entry = diskUsageByTab.get(key);
      if (entry !== undefined) startDiskUsageScan(entry.paneId, entry.tabId, entry.location);
    },
    stopDiskUsage,
  };

  function replaceWorkspace(next: WorkspaceProjection): void {
    workspace = next;
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
    if (workspace === undefined || workspace.id !== next.id) {
      replaceWorkspace(next);
      return;
    }
    const localWorkspace = workspace;
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
    const pending = settingsUpdateQueue.then(async () => {
      if (currentSettings === undefined) {
        throw new Error('Cannot update settings: settings have not loaded yet.');
      }
      currentSettings = await client.updateSettings(update(currentSettings));
    });
    settingsUpdateQueue = pending.catch(() => undefined);
    return pending
      .catch((error: unknown) => {
        console.error('Failed to persist a settings update.', error);
        throw error;
      })
      .finally(() => m.redraw());
  }

  async function activatePane(client: FileManagerClient, paneId: PaneId): Promise<void> {
    if (workspace === undefined || workspace.activePaneId === paneId) {
      return;
    }
    const previousWorkspace = workspace;
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
      if (workspace?.revision === previousWorkspace.revision) replaceWorkspace(previousWorkspace);
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
    if (workspace === undefined) return;
    const target = await client
      .resyncWorkspace?.(workspace.id, targetWorkspaceId)
      .catch(() => undefined);
    if (target === undefined || workspace === undefined) return;
    // Any resync - the default source, a from-scratch session's first sync, or an explicit
    // "Update" onto a different saved workspace - relinks this window to whichever named
    // workspace it just synced into, mirroring what the backend already did to the persisted
    // ephemeral record, so a later default resync (or "New Window" from this window) keeps
    // targeting that same workspace.
    if (workspace.forkedFrom !== target.id) {
      workspace = { ...workspace, forkedFrom: target.id };
    }
    workspaceController.refreshWorkspaceSummaries();
    m.redraw();
  }

  function selectTab(client: FileManagerClient, paneId: PaneId, tabId: TabId): void {
    if (workspace?.panesById[paneId]?.activeTabId === tabId) {
      // Already the active tab - no tab switch, but still worth refreshing: the user is
      // deliberately revisiting this listing (e.g. clicking back onto it after an external change
      // like a browser download landed while it sat idle), so `activateTab` refreshes it too.
      void activatePane(client, paneId).catch(() => undefined);
      tabController.activateTab(paneId, tabId);
      return;
    }
    tabController.activateTab(paneId, tabId);
  }

  function updateLayout(client: FileManagerClient, layout: WorkspaceLayout): void {
    if (workspace === undefined) {
      return;
    }
    void dispatchWorkspaceCommand(
      client,
      {
        type: 'updateLayout',
        workspaceId: workspace.id,
        layout,
        expectedRevision: workspace.revision,
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
    const current = workspace;
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
    getOperationCentreVisible: () => workspace?.operationCentre.visible === true,
    toggleOperationCentre,
    getOperations: () => operations,
    setOperations: (next) => {
      operations = next;
    },
    getPendingConflict: () => pendingConflict,
    setPendingConflict: (conflict) => {
      pendingConflict = conflict;
    },
    getConnections: () => connections,
    setConnections: (conns) => {
      connections = conns;
    },
    getConnectionsManagerOpen: () => connectionsManagerOpen,
    setConnectionsManagerOpen: (open) => {
      connectionsManagerOpen = open;
    },
    getFindFilesOpen: () => findFilesOpen,
    getFindFilesRoot: () => findFilesRoot,
    getFindFilesError: () => findFilesError,
    getCloseTabConfirmation: () => closeTabConfirmation,
    setCloseTabConfirmation: (conf) => {
      closeTabConfirmation = conf;
    },
    getDialogs: () => dialogs,
    getFormatSettings: () => currentEntryFormatSettings,
    getFindFilesController: () => findFilesController,
    getTabController: () => tabController,
    getOpsController: () => opsController,
    getActiveDirectoryLocation: () => activeDirectory()?.location,
    getActivePaneId: () => activeDirectory()?.paneId,
    navigateActiveLocation: async (location) => {
      const paneId = activeDirectory()?.paneId;
      if (paneId !== undefined) await navigation.navigate(paneId, location);
    },
    getFocusPane: () => focusPane,
    getSettings: () => currentSettings,
    updateSettings: (update) => updateLocationSettings(attrsClient, update),
    openEditorForCreatedFile: (location, name) => {
      const active = activeDirectory();
      if (active === undefined) return;
      refetchAffectedPanes(active.paneId);
      openEditor(attrsClient, active.paneId, {
        id: crypto.randomUUID(),
        location,
        name,
        kind: 'file',
        hidden: false,
        readOnly: false,
        metadataRevision: 0,
      });
    },
    getFinderTagsLoader: () => finderTagsLoader,
    cancelAutoDismiss,
    rememberDismissedOperation,
    hasDismissedOperations: () => dismissedOperationIds.size > 0,
    showAllOperations,
    refetchAffectedPanes,
    redraw: () => m.redraw(),
  };

  return {
    oninit: ({ attrs }) => {
      attrsClient = attrs.client;
      // Composition seam (task 0153, controller-registry.ts): every shell-lifetime controller is
      // constructed and torn down through this one registry instead of by-hand `let` +
      // `create*Controller(...)` + a matching teardown call hand-placed in `onremove`.
      const shellControllers = buildControllers({
        ops: { create: () => createOperationsController(attrs.client) },
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
        checksum: { create: () => createChecksumController(checksumControllerContext) },
        actionCommand: {
          create: () => createActionCommandController(actionCommandControllerContext),
        },
        paneContent: { create: () => createPaneContentBuilder(paneContentBuilderContext) },
        navigation: {
          create: () =>
            createNavigationController({
              client: attrs.client,
              getWorkspace: () => workspace,
              replaceWorkspace: (next) => replaceWorkspace(next),
              onLocationVisited: (workspaceId, location) => {
                unavailableLocations.delete(`${location.providerId}:${location.uri}`);
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
                unavailableLocations.add(`${location.providerId}:${location.uri}`);
                m.redraw();
              },
              requestArchivePassword: (location, invalid) => {
                dialogs.getState().pendingArchiveCredential?.resolve(false);
                dialogs.clearArchiveCredential();
                return new Promise<boolean>((resolve) => {
                  dialogs.setPendingArchiveCredential({ location, invalid, resolve });
                  m.redraw();
                });
              },
              // Mirrors the same "on and the directory actually reports git status" gate
              // `pane-content-builder.ts` uses to decide whether to render the column - here it
              // decides whether to *ask the backend to compute* git status at all, so a hidden
              // column costs nothing server-side either. Read fresh on every request rather than
              // snapshotted, since `currentSettings` can change between navigations.
              getShowGitStatusColumn: () =>
                currentSettings?.defaultColumns.includes('core.gitStatus') ?? false,
              updatePane: (paneId, tabId, view, preferredCursorName) => {
                const key = tabKey(paneId, tabId);
                const previous = directories.get(key);
                directories.set(key, respectSystemLocationReadOnly(view, systemLocations));
                if (previous !== undefined && previous.location?.uri === view.location?.uri) {
                  reconcileSelectionAfterEntryChange(paneId, tabId, previous.entries, view.entries);
                }
                if (view.entries.length === 0) {
                  // The table still renders a synthetic ".." row (via `withParentEntry`) for any
                  // location that isn't a filesystem root, even when the directory itself is
                  // empty - so an empty directory must not be treated as "nothing to put the
                  // cursor on": the cursor still needs to land on that ".." row when one is
                  // rendered.
                  const parentEntry =
                    view.location === undefined
                      ? undefined
                      : withParentEntry(pathFromUri(view.location.uri), [])[0];
                  selections.set(key, {
                    selectedEntryIds: [],
                    ...(parentEntry === undefined
                      ? {}
                      : { cursorEntryId: parentEntry.id, anchorEntryId: parentEntry.id }),
                  });
                } else if (
                  selections.get(key)?.cursorEntryId === undefined ||
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
                  const tab = workspace?.panesById[paneId]?.tabsById[tabId];
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
                  selections.set(key, {
                    selectedEntryIds: [],
                    ...(firstEntry === undefined
                      ? {}
                      : { cursorEntryId: firstEntry.id, anchorEntryId: firstEntry.id }),
                  });
                }
                m.redraw();
              },
            }),
          dispose: (controller) => controller.dispose(),
        },
      });
      opsController = shellControllers.instances.ops;
      workspaceController = shellControllers.instances.workspace;
      tabController = shellControllers.instances.tab;
      settingsController = shellControllers.instances.settings;
      findFilesController = shellControllers.instances.findFiles;
      comparisonController = shellControllers.instances.comparison;
      checksumController = shellControllers.instances.checksum;
      actionCommandController = shellControllers.instances.actionCommand;
      paneContentBuilder = shellControllers.instances.paneContent;
      navigation = shellControllers.instances.navigation;
      disposeShellControllers = shellControllers.dispose;
      keybindingRuntime = attrs.runtime === 'http' ? 'browser' : 'desktop';
      runtimeKind = attrs.runtime;
      systemThemeQuery?.addEventListener('change', handleSystemThemeChange);
      window.addEventListener('focus', handleWindowFocus);
      document.addEventListener('keydown', updateFunctionKeyModifiers);
      document.addEventListener('keyup', updateFunctionKeyModifiers);
      window.addEventListener('blur', resetFunctionKeyModifiers);
      appState = applyAppPatches(
        createInitialAppState(attrs.runtime),
        connectionPatch({ status: attrs.client.connection.get() }),
      );
      // Specification §26 keeps settings on the backend rather than in browser
      // storage, so the theme manager's own localStorage persistence stays off;
      // task 0030 restores the theme from the settings service instead.
      ThemeManager.setUseLocalStorage(false);
      ThemeManager.initialize(theme);
      void loadSettings(attrs.client);
      void attrs.client
        .listActions()
        .then((actions) => {
          registeredActions = actions;
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
                    nativeMenuChannelReady = true;
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
          plugins = listed;
          if (currentSettings !== undefined) applyIconTheme(currentSettings.iconTheme);
          m.redraw();
        })
        .catch(() => undefined);
      void workspaceController.loadWorkspace();
      void Promise.resolve()
        .then(() => attrs.client.listOperations())
        .then((listed) => {
          if (!removed) {
            // History is loaded from a PAST session - the user never watched these
            // run, so an auto-dismissible one (completed/cancelled/interrupted) would
            // only flash and vanish a few seconds later for no reason. Only surface
            // ones that still need attention (failed) or are still genuinely active.
            const relevant = listed.filter(
              (operation) =>
                !shouldAutoDismissOperation(operation) && !dismissedOperationIds.has(operation.id),
            );
            operations = createOperationsState(relevant);
            for (const operation of relevant) {
              if (!operationIsActive(operation)) continue;
              const elapsed = Date.now() - Date.parse(operation.createdAt);
              scheduleOperationCentreOpen(operation.id, Math.max(0, 3_000 - elapsed));
            }
            m.redraw();
          }
        })
        .catch(() => undefined);
      unsubscribeConnection = attrs.client.connection.subscribe((status) => {
        if (appState !== undefined) {
          appState = applyAppPatches(appState, connectionPatch({ status }));
        }
        m.redraw();
      });
      unsubscribeResynchronise = attrs.client.onResynchronise(() => refetchAffectedPanes());
      void attrs.client.subscribe(handleBackendEvent).then((unsubscribe) => {
        if (removed) unsubscribe();
        else unsubscribeEvents = unsubscribe;
      });
    },

    onremove: () => {
      removed = true;
      dialogs.getState().pendingArchiveCredential?.resolve(false);
      dialogs.clearArchiveCredential();
      systemThemeQuery?.removeEventListener('change', handleSystemThemeChange);
      window.removeEventListener('focus', handleWindowFocus);
      document.removeEventListener('keydown', updateFunctionKeyModifiers);
      document.removeEventListener('keyup', updateFunctionKeyModifiers);
      window.removeEventListener('blur', resetFunctionKeyModifiers);
      if (operationFrame !== undefined) cancelAnimationFrame(operationFrame);
      for (const timer of autoDismissTimers.values()) clearTimeout(timer);
      autoDismissTimers.clear();
      for (const timer of operationCentreOpenTimers.values()) clearTimeout(timer);
      operationCentreOpenTimers.clear();
      workspaceRequest?.abort();
      unsubscribeEvents?.();
      unsubscribeNativeFileDrops?.();
      unsubscribeConnection?.();
      unsubscribeResynchronise?.();
      attrsClient.disconnect();
      disposeShellControllers?.();
      document.documentElement.style.removeProperty('--fm-font-size');
      document.documentElement.style.removeProperty('--fm-row-height');
    },

    view: ({ attrs }) => {
      syncNativeMenu();
      if (treeSidebarOpen) syncDirectoryTreeToActiveLocation();
      currentEntryFormatSettings = attrs.entryFormatSettings ?? loadedEntryFormatSettings;
      const pendingDelete = Object.values(operations.byId).find(
        (operation) =>
          operation?.kind === 'delete' && operation.state === 'waitingForConflictResolution',
      );
      // macOS's overlay title bar (spec follow-up) keeps the native traffic lights, but
      // draws our own centred title in a reserved CSS row instead of the OS title text
      // (hidden via hiddenTitle) -- this is what makes the frame colour match, since a
      // plain "Transparent" title bar still let the OS render its own vibrancy behind it.
      // The web build doesn't need this: the browser tab already shows the title.
      const isMacOverlay = runtimeKind === 'tauri' && platform === 'macos';
      return m(
        '.fm-app-shell',
        { 'data-mac-titlebar-overlay': isMacOverlay ? 'true' : undefined },
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
                  spec: windowsNativeMenuSpec,
                  onAction: (id) => dispatchNativeMenuAction(nativeMenuDispatchContext, id),
                  onRole: (role) => {
                    const win = getCurrentWindow();
                    if (role === 'minimize') void win.minimize();
                    if (role === 'zoom') void win.toggleMaximize();
                    if (role === 'quit') void win.close();
                    if (role === 'about') {
                      aboutDialogOpen = true;
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
                    '−',
                  ),
                  m(
                    'button',
                    {
                      type: 'button',
                      'aria-label': t('shell', 'maximize'),
                      onclick: () => void getCurrentWindow().toggleMaximize(),
                    },
                    '□',
                  ),
                  m(
                    'button.fm-windows-titlebar-close',
                    {
                      type: 'button',
                      'aria-label': t('button', 'close'),
                      onclick: () => void getCurrentWindow().close(),
                    },
                    '×',
                  ),
                ]),
              ])
            : undefined,
          isMacOverlay
            ? m('.fm-titlebar-spacer', { 'data-tauri-drag-region': '' }, [
                m('span.fm-titlebar-label', t('shell', 'title')),
              ])
            : null,
          m('.fm-workspace-toolbar', [
            m('.fm-navigation-controls', { 'aria-label': t('shell', 'activePaneNavigation') }, [
              tooltip(
                t('shell', 'back'),
                m(
                  IconButton,
                  {
                    disabled:
                      workspace?.panesById[workspace.activePaneId]?.tabsById[
                        workspace.panesById[workspace.activePaneId]?.activeTabId ?? ''
                      ]?.canNavigateBack !== true,
                    'aria-label': t('shell', 'back'),
                    onclick: () => void navigation.back(workspace?.activePaneId ?? ''),
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
                      workspace?.panesById[workspace.activePaneId]?.tabsById[
                        workspace.panesById[workspace.activePaneId]?.activeTabId ?? ''
                      ]?.canNavigateForward !== true,
                    'aria-label': t('shell', 'forward'),
                    onclick: () => void navigation.forward(workspace?.activePaneId ?? ''),
                  },
                  arrowRightIcon(),
                ),
              ),
              tooltip(
                t('shell', 'parentDirectory'),
                m(
                  IconButton,
                  {
                    disabled: workspace === undefined,
                    'aria-label': t('shell', 'parentDirectory'),
                    onclick: () => void navigation.parent(workspace?.activePaneId ?? ''),
                  },
                  cornerLeftUpIcon(),
                ),
              ),
            ]),
            tooltip(
              t('shell', 'findFiles'),
              m(
                IconButton,
                {
                  disabled: activeDirectory() === undefined,
                  'aria-label': t('shell', 'findFiles'),
                  onclick: () => {
                    findFilesController.openFindFiles();
                  },
                },
                searchIcon(),
              ),
            ),
            tooltip(
              t('shell', 'comparePanes'),
              m(
                IconButton,
                {
                  disabled: (workspace?.paneOrder.length ?? 0) < 2,
                  'aria-label': t('shell', 'comparePanes'),
                  onclick: () => comparisonController.startComparison('sizeAndTimestamp'),
                },
                compareIcon(),
              ),
            ),
            tooltip(
              t('shell', 'commandPalette'),
              m(
                IconButton,
                {
                  className: 'fm-command-palette-trigger',
                  disabled: registeredActions.length === 0,
                  'aria-label': t('shell', 'commandPalette'),
                  onclick: () => {
                    commandPaletteOpen = true;
                  },
                },
                commandIcon(),
              ),
            ),
            tooltip(
              t('shell', 'workspaceSwitcherLabel', { name: workspace?.name ?? t('shell', 'none') }),
              m(
                IconButton,
                {
                  className: 'fm-workspace-switcher-button',
                  'aria-label': t('shell', 'workspaceSwitcherLabel', {
                    name: workspace?.name ?? t('shell', 'none'),
                  }),
                  onclick: () => {
                    if (workspaceDisclosureElement !== undefined) {
                      workspaceDisclosureElement.open = !workspaceDisclosureElement.open;
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
                  workspaceDisclosureElement = dom as HTMLDetailsElement;
                },
                onremove: () => {
                  workspaceDisclosureElement = undefined;
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
                      summaries: sortWorkspaceSummaries(workspaceSummaries),
                      // An ephemeral window's own id is never listed here (ephemeral workspaces
                      // are excluded from the switcher) - the row that should read as "current"
                      // is the named workspace it was forked from, if any. A non-ephemeral
                      // window (the main/dock window) already is that named workspace.
                      activeWorkspaceId:
                        workspace === undefined
                          ? undefined
                          : workspace.ephemeral
                            ? workspace.forkedFrom
                            : workspace.id,
                      error: workspaceActionError,
                      onSwitch: (workspaceId) => {
                        void switchWorkspace(workspaceId);
                      },
                      onCreate: () => workspaceController.createWorkspaceAction(),
                      onRename: (workspaceId, name) =>
                        workspaceController.renameWorkspaceAction(workspaceId, name),
                      onDelete: (workspaceId) =>
                        workspaceController.deleteWorkspaceAction(workspaceId),
                      ...(attrsClient.openWorkspaceWindow === undefined
                        ? {}
                        : {
                            onOpenInNewWindow: (workspaceId: WorkspaceId) => {
                              void attrsClient.openWorkspaceWindow?.(workspaceId);
                              if (workspaceDisclosureElement !== undefined) {
                                workspaceDisclosureElement.open = false;
                              }
                            },
                          }),
                      ...(attrsClient.resyncWorkspace === undefined || workspace === undefined
                        ? {}
                        : {
                            onUpdate: (workspaceId: WorkspaceId) => {
                              void resyncWorkspace(attrsClient, workspaceId);
                            },
                          }),
                    }),
                  ],
                ),
              ],
            ),
            tooltip(
              t('shell', 'operationCentre'),
              m(
                IconButton,
                {
                  className: 'fm-operation-centre-button',
                  disabled: workspace === undefined,
                  'aria-label': t('shell', 'operationCentre'),
                  'aria-pressed': String(workspace?.operationCentre.visible === true),
                  onclick: toggleOperationCentre,
                },
                listIcon(),
              ),
            ),
            tooltip(
              t('shell', 'diagnostics'),
              m(
                IconButton,
                {
                  className: 'fm-diagnostics-button',
                  'aria-label': t('shell', 'diagnostics'),
                  onclick: () => {
                    if (diagnosticsDisclosureElement === undefined) return;
                    diagnosticsDisclosureElement.open = !diagnosticsDisclosureElement.open;
                    diagnosticsDialogOpen = diagnosticsDisclosureElement.open;
                    m.redraw();
                  },
                },
                activityIcon(),
              ),
            ),
            m(
              'details.fm-diagnostics-disclosure',
              {
                oncreate: ({ dom }) => {
                  diagnosticsDisclosureElement = dom as HTMLDetailsElement;
                },
                onremove: () => {
                  diagnosticsDisclosureElement = undefined;
                },
              },
              [
                m('summary.fm-disclosure-summary-hidden'),
                m(
                  '.fm-diagnostics-editor',
                  {
                    role: 'dialog',
                    'aria-label': t('shell', 'systemDiagnostics'),
                    onclick: (event: MouseEvent) => {
                      if (event.target === event.currentTarget) {
                        if (diagnosticsDisclosureElement !== undefined)
                          diagnosticsDisclosureElement.open = false;
                        diagnosticsDialogOpen = false;
                      }
                    },
                  },
                  [
                    m('.fm-settings-editor-panel', [
                      m('.fm-settings-editor-heading', [
                        m('strong', t('shell', 'systemDiagnostics')),
                        m(
                          'button',
                          {
                            type: 'button',
                            'aria-label': t('shell', 'closeDiagnostics'),
                            onclick: () => {
                              if (diagnosticsDisclosureElement !== undefined)
                                diagnosticsDisclosureElement.open = false;
                              diagnosticsDialogOpen = false;
                            },
                          },
                          closeIcon(),
                        ),
                      ]),
                      diagnosticsDialogOpen
                        ? m(DiagnosticsViewComponent, { client: attrsClient })
                        : undefined,
                    ]),
                  ],
                ),
              ],
            ),
            tooltip(
              t('shell', 'openSettings'),
              m(
                IconButton,
                {
                  className: 'fm-settings-button',
                  'aria-label': t('shell', 'settings'),
                  onclick: () => {
                    if (settingsDisclosureElement === undefined) return;
                    settingsDisclosureElement.open = !settingsDisclosureElement.open;
                    settingsDialogOpen = settingsDisclosureElement.open;
                    if (!settingsDialogOpen && currentSettings !== undefined) {
                      applyAppearance(currentSettings);
                    }
                    m.redraw();
                  },
                },
                settingsIcon(),
              ),
            ),
            m(
              'details.fm-settings-disclosure',
              {
                oncreate: ({ dom }) => {
                  settingsDisclosureElement = dom as HTMLDetailsElement;
                },
                onremove: () => {
                  settingsDisclosureElement = undefined;
                },
                ontoggle: (event: Event) => {
                  const open = (event.currentTarget as HTMLDetailsElement).open;
                  if (settingsDialogOpen === open) return;
                  settingsDialogOpen = open;
                  if (!open && currentSettings !== undefined) {
                    applyAppearance(currentSettings);
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
                        closeSettingsDialog();
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
                            onclick: () => closeSettingsDialog(),
                          },
                          closeIcon(),
                        ),
                      ]),
                      currentSettings === undefined
                        ? m('p', t('shell', 'loading'))
                        : settingsDialogOpen
                          ? m(SettingsEditor, {
                              settings: currentSettings,
                              actions: localisedRegisteredActions(),
                              platform,
                              runtime: keybindingRuntime,
                              plugins,
                              onPreview: (draft: Settings) => {
                                applyAppearance(draft);
                                m.redraw();
                              },
                              onSave: async (draft: Settings) => {
                                const showHiddenChanged =
                                  currentSettings !== undefined &&
                                  currentSettings.showHiddenFiles !== draft.showHiddenFiles;
                                await updateLocationSettings(attrs.client, (latest) => ({
                                  ...draft,
                                  multiRenamePresets: latest.multiRenamePresets,
                                }));
                                applyAppearance(draft);
                                closeSettingsDialog();
                                if (showHiddenChanged) {
                                  void applyShowHiddenFilesToAllTabs(
                                    attrs.client,
                                    draft.showHiddenFiles,
                                  );
                                }
                              },
                              onCancel: () => {
                                if (currentSettings !== undefined) applyAppearance(currentSettings);
                                closeSettingsDialog();
                              },
                              onTogglePlugin: (pluginId: PluginId, enabled: boolean) =>
                                attrs.client.setPluginEnabled(pluginId, enabled),
                              onRequestPluginLogs: (
                                pluginId: PluginId,
                              ): Promise<readonly PluginLogEntry[]> =>
                                attrs.client.getPluginLogs(pluginId),
                            })
                          : undefined,
                    ]),
                  ],
                ),
              ],
            ),
          ]),
          m('main.fm-workspace', [
            (() => {
              if (!treeSidebarOpen || treeRootLocation === undefined) return undefined;
              const treeRoot = treeRootLocation;
              const activeLocationUri = activeDirectory()?.location.uri;
              return m('.fm-directory-tree-sidebar', [
                m('.fm-directory-tree-header', [
                  m('span', t('tree', 'directoryTree')),
                  m(
                    'button.fm-directory-tree-close',
                    {
                      type: 'button',
                      'aria-label': t('tree', 'toggleSidebar'),
                      onclick: toggleDirectoryTree,
                    },
                    closeIcon({ size: 13 }),
                  ),
                ]),
                m(DirectoryTree, {
                  root: { location: treeRoot, name: treeRootName(treeRoot) },
                  state: treeState,
                  ...(activeLocationUri === undefined ? {} : { activeLocationUri }),
                  onToggleExpand: (location: Location) => {
                    toggleTreeNode(location);
                    m.redraw();
                  },
                  onActivate: (location: Location) => {
                    const active = activeDirectory();
                    if (active === undefined) return;
                    void navigation?.navigate(active.paneId, location);
                  },
                  onTabOut: (direction) => {
                    const paneOrder = workspace?.paneOrder;
                    if (paneOrder === undefined || paneOrder.length === 0) return;
                    const targetPaneId =
                      direction === 1 ? paneOrder[0] : paneOrder[paneOrder.length - 1];
                    if (targetPaneId !== undefined)
                      globalKeydownHandlerContext.focusPane(targetPaneId);
                  },
                  registerFocus: (focus) => {
                    focusDirectoryTree = focus;
                  },
                } satisfies DirectoryTreeAttrs),
              ]);
            })(),
            workspace === undefined
              ? m('.fm-workspace-loading', workspaceError ?? t('shell', 'loading'))
              : m(WorkspaceLayoutView, {
                  workspace,
                  paneContent: (paneId) =>
                    paneContentBuilder(
                      attrs.client,
                      attrs.entryFormatSettings ?? loadedEntryFormatSettings,
                      paneId,
                    ),
                  onActivatePane: (paneId) =>
                    void activatePane(attrs.client, paneId).catch(() => undefined),
                  onUpdateLayout: (layout) => updateLayout(attrs.client, layout),
                  onSelectTab: (paneId, tabId) => selectTab(attrs.client, paneId, tabId),
                  onCloseTab: (paneId, tabId) => tabController.requestCloseTab(paneId, tabId),
                  onNewTab: (paneId) => tabController.openTab(paneId),
                  onMoveTab: (sourcePaneId, tabId, targetPaneId, targetIndex) =>
                    moveTab(attrs.client, sourcePaneId, tabId, targetPaneId, targetIndex),
                  onFocusTerminal: () => focusTerminal?.() ?? false,
                  onFocusViewer: () => {
                    const paneId = openViewerPaneId();
                    if (paneId === undefined) return false;
                    focusViewer(paneId);
                    m.redraw();
                    return true;
                  },
                  onPaneCycleBoundary: () => {
                    if (!treeSidebarOpen) return false;
                    focusDirectoryTree?.();
                    return true;
                  },
                  registerFlush: (flush) => {
                    flushPendingLayoutUpdate = flush;
                  },
                  registerFocusPane: (focus) => {
                    focusPane = focus;
                  },
                  searchPresentationForLocationUri: (uri) =>
                    findFilesPresentationsByLocationUri.get(uri),
                  onRefreshSearch: (uri, paneId) => findFilesController.refreshSearch(uri, paneId),
                }),
            // Checksum and duplicate panels sit below the panes, visible only
            // while their job/scan is tracked (task 0077).
            checksumState.jobId !== undefined &&
              m(ChecksumResultsView, {
                algorithms: checksumState.algorithms,
                entries: checksumState.entries,
                totalEntries: checksumState.totalEntries,
                isComplete: checksumState.isComplete,
                isCancelled: checksumState.isCancelled,
                ...(checksumState.verification === undefined
                  ? {}
                  : { verification: checksumState.verification }),
                ...(checksumState.error === undefined ? {} : { error: checksumState.error }),
                onCopy: (algorithm) => {
                  void checksumController.copyChecksums(algorithm).then((content) => {
                    if (content !== undefined) void navigator.clipboard?.writeText(content);
                  });
                },
                ...(checksumState.savedTo === undefined ? {} : { savedTo: checksumState.savedTo }),
                suggestedFileName: (algorithm) => checksumController.suggestedFileName(algorithm),
                onSave: (algorithm, fileName) => {
                  void checksumController.saveChecksumFile(algorithm, fileName);
                },
                onVerify: (content) => checksumController.verifyAgainst(content),
                onCancel: () => checksumController.cancelChecksums(),
                onClose: () => checksumController.closeChecksums(),
              }),
            duplicateState.scanId !== undefined &&
              m(DuplicateReviewView, {
                groups: duplicateState.groups,
                isComplete: duplicateState.isComplete,
                isCancelled: duplicateState.isCancelled,
                warningsCount: duplicateState.warningsCount,
                selectedUris: duplicateState.selectedUris,
                totalReclaimableBytes: totalReclaimableBytes(duplicateState),
                ...(duplicateState.error === undefined ? {} : { error: duplicateState.error }),
                isLastCopy: (uri) => wouldDeleteEveryCopy(duplicateState, uri),
                onToggle: (uri) => checksumController.toggleDuplicateSelection(uri),
                onDeleteSelected: () => checksumController.deleteSelectedDuplicates(),
                onCancel: () => checksumController.cancelDuplicateScan(),
                onClose: () => checksumController.closeDuplicates(),
              }),
          ]),
          runtimeKind === 'tauri'
            ? m(TerminalDrawer, {
                open: isTerminalVisible(openTerminalTabKeys, activeTerminalTabKey()),
                tabKey: activeTerminalTabKey(),
                location: activeDirectory()?.location,
                client: tauriTerminalClient,
                onToggle: globalKeydownHandlerContext.toggleTerminal,
                onSwitchPane: () => {
                  if (workspace === undefined) return;
                  const index = workspace.paneOrder.indexOf(workspace.activePaneId);
                  const nextPaneId = workspace.paneOrder[(index + 1) % workspace.paneOrder.length];
                  if (nextPaneId !== undefined) focusPane?.(nextPaneId);
                },
                onCycleTab: (direction) => {
                  if (workspace === undefined) return;
                  const paneId = workspace.activePaneId;
                  tabController.cycleTab(paneId, direction);
                  focusPane?.(paneId);
                },
                onFocusFolder: () => {
                  if (workspace !== undefined) focusPane?.(workspace.activePaneId);
                },
                registerFocus: (focus) => {
                  focusTerminal = focus;
                },
                registerDisposeTab: (dispose) => {
                  disposeTerminalTab = dispose;
                },
              })
            : undefined,
          clipboardMessage === undefined
            ? undefined
            : m('.fm-clipboard-message', { role: 'alert' }, clipboardMessage),
          m(CommandPalette, {
            open: commandPaletteOpen,
            actions: actionsWithFavourites(),
            recency: commandPaletteRecency,
            context: actionCommandController.actionContext(),
            availabilityContext: actionCommandController.commandAvailabilityContext(),
            onClose: () => {
              commandPaletteOpen = false;
            },
            onInvoke: actionCommandController.invokePaletteAction,
          }),
          m(DirectoryContextMenu, {
            open: contextMenu !== undefined,
            x: contextMenu?.x ?? 0,
            y: contextMenu?.y ?? 0,
            actions:
              contextMenu === undefined
                ? []
                : menuActionsForContext(
                    localisedRegisteredActions(),
                    actionCommandController.commandAvailabilityContext(
                      contextMenu.entries,
                      contextMenu.paneId,
                    ),
                  ),
            onClose: () => {
              contextMenu = undefined;
            },
            onInvoke: actionCommandController.invokeContextMenuAction,
            ...(platformContextMenuSupported &&
            contextMenu !== undefined &&
            contextMenu.entries.length > 0 &&
            contextMenu.entries.every((entry) => entry.location.providerId === 'local') &&
            (platform === 'macos' || platform === 'windows')
              ? {
                  platformSubmenu: {
                    title:
                      platform === 'macos'
                        ? t('contextMenu', 'services')
                        : t('contextMenu', 'sendTo'),
                    onOpen: () => {
                      const locations = contextMenu?.entries.map((entry) => entry.location) ?? [];
                      void attrsClient.showPlatformContextMenu(locations).catch(() => {
                        toast({ html: t('contextMenu', 'platformMenuFailed') });
                      });
                    },
                  },
                }
              : {}),
          }),
          m(ShortcutsHelpDialog, {
            open: shortcutsHelpOpen,
            actions: localisedRegisteredActions(),
            keybindings: currentSettings?.keybindings ?? {},
            platform,
            runtime: keybindingRuntime,
            onClose: () => {
              shortcutsHelpOpen = false;
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
            isOpen: aboutDialogOpen,
            closeOnEsc: true,
            onToggle: (open: boolean) => {
              if (!open) aboutDialogOpen = false;
            },
          }),
          ...renderAppDialogs(attrs.client, pendingDelete, appDialogsContext),
          m(
            '.fm-function-key-bar',
            footerFunctionKeyBindings(
              localisedRegisteredActions(),
              currentSettings?.keybindings ?? {},
              {
                scope: 'table',
                platform,
                runtime: attrs.runtime === 'http' ? 'browser' : 'desktop',
              },
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
                  actionCommandController.commandAvailabilityContext(),
                ).available,
              functionKeyModifiers,
            ).map((binding) =>
              m(
                'span.fm-function-key',
                {
                  key: binding.actionId,
                  role: 'button',
                  tabindex: binding.actionAvailable ? 0 : -1,
                  'aria-disabled': binding.actionAvailable ? undefined : 'true',
                  onclick: binding.actionAvailable
                    ? () => invokeFunctionKeyShortcut(binding.shortcut)
                    : undefined,
                },
                `${binding.key} ${functionKeyTitle(binding)}`,
              ),
            ),
          ),
        ],
      );
    },
  };
};
