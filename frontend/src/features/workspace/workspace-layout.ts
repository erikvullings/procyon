import m, { type FactoryComponent, type Vnode } from 'mithril';
import { dispatchKeybinding, type KeybindingRuntime } from '../../keybindings/dispatcher';
import type {
  ActionDescriptor,
  Connection,
  EntryId,
  EntrySummary,
  FavouriteLocation,
  LoadingState,
  Location,
  PaneId,
  SavedSearch,
  SortDescriptor,
  SystemLocation,
  TabId,
  Volume,
  VolumeCapacity,
  WorkspaceLayout,
  WorkspaceProjection,
} from '../../models';
import {
  connectionForLocation,
  isBrowsable,
  remoteRootLocation,
} from '../connections/connections-model';
import type { GridIconSize } from '../directory-table/directory-grid';
import type {
  ColumnWidthEntry,
  DirectoryColumnDescriptor,
} from '../directory-table/directory-table';
import type { FinderTagsLoader } from '../directory-table/finder-tags-loader';
import type { NativeIconLoader } from '../directory-table/native-icon-loader';
import type { ThumbnailLoader } from '../directory-table/thumbnail-loader';
import type { EntryFormatSettings } from '../entry-formatting/entry-formatting';
import type {
  DirectorySummaryAttrs,
  FavouritesAttrs,
  FilterAttrs,
  PaneNavigationAttrs,
  TableConfigAttrs,
} from '../panes/pane';
import { Pane } from '../panes/pane';
import type { SearchPresentation } from '../search/search-presentation';
import type { SelectionPlatform } from '../selection/keybindings';
import type { SelectionAction } from '../selection/selection';
import './workspace-layout.css';

const MIN_PANE_WIDTH = 240;

/** Directory-session and view data supplied for one workspace pane. */
export interface WorkspacePaneContent {
  readonly state: LoadingState;
  readonly entries: readonly EntrySummary[];
  readonly selectedEntryIds: ReadonlySet<EntryId>;
  readonly cutEntryIds: ReadonlySet<EntryId>;
  readonly sortLabel: string;
  readonly sort: readonly SortDescriptor[];
  readonly hasMore?: boolean;
  readonly totalEntryCount: number;
  readonly totalKnownEntries?: number;
  readonly totalKnownSize?: number;
  readonly totalKnownFileCount?: number;
  /** Backing volume's total/available capacity, when known (task 0096). */
  readonly volumeCapacity?: VolumeCapacity;
  readonly hiddenSelectedCount: number;
  readonly filterOpen: boolean;
  readonly filterQuery: string;
  readonly formatSettings?: EntryFormatSettings;
  readonly pluginColumns?: readonly DirectoryColumnDescriptor[];
  /** Restricts non-mandatory columns to this set; see `DirectoryTableAttrs.visibleColumnIds`. */
  readonly visibleColumnIds?: ReadonlySet<string>;
  /** Shows the Git-status column; hidden unless enabled and the directory is inside a git repo. */
  readonly showGitStatusColumn?: boolean;
  readonly nativeIconLoader?: NativeIconLoader;
  readonly thumbnailLoader?: ThumbnailLoader;
  readonly finderTagsLoader?: FinderTagsLoader;
  readonly cursorIndex?: number;
  readonly platform: SelectionPlatform;
  readonly keybindingRuntime?: KeybindingRuntime;
  readonly actions?: readonly ActionDescriptor[];
  readonly keybindingOverrides?: Readonly<Record<string, string>>;
  readonly location?: Location;
  readonly defaultFavouriteLabel?: string;
  readonly currentLocationIsSavedSearch?: boolean;
  readonly favouriteLocations?: readonly FavouriteLocation[];
  readonly recentLocations?: readonly Location[];
  readonly savedSearches?: readonly SavedSearch[];
  readonly systemLocations?: readonly SystemLocation[];
  readonly systemLocationsError?: string;
  readonly onRetrySystemLocations?: () => void | Promise<void>;
  /** Currently mounted local/removable/disk-image volumes (task 0144). */
  readonly volumes?: readonly Volume[];
  readonly volumesError?: string;
  readonly onRetryVolumes?: () => void | Promise<void>;
  /** Saved application-managed connections shown in the `SERVERS` group (task 0103). */
  readonly connections?: readonly Connection[];
  /** Opens the connections manager (add/edit/delete/connect/disconnect/test, task 0103). */
  readonly onManageConnections?: () => void;
  readonly onOpenSavedSearch?: (saved: SavedSearch) => void;
  /** Refreshes the saved-connections list before opening favourites so status glyphs stay fresh. */
  readonly onRefreshConnections?: () => void | Promise<void>;
  readonly unavailableLocations?: ReadonlySet<string>;
  readonly onNavigateLocation?: (location: Location) => void | Promise<void>;
  readonly onAddFavourite?: (label: string, location: Location) => void | Promise<void>;
  readonly onDeleteFavourite?: (location: Location) => void | Promise<void>;
  readonly onReorderFavourites?: (from: number, to: number) => void | Promise<void>;
  readonly onNavigate: (path: string) => void | Promise<void>;
  readonly onBack: () => void | Promise<void>;
  readonly onForward: () => void | Promise<void>;
  readonly onParent: () => void | Promise<void>;
  readonly onOpenEntry: (entry: EntrySummary) => void | Promise<void>;
  readonly onSelectionAction: (action: SelectionAction) => void;
  readonly onRetry: () => void | Promise<void>;
  readonly onLoadNextPage: () => void | Promise<void>;
  readonly onSortChange: (sort: readonly SortDescriptor[]) => void;
  /** Table vs. thumbnail grid, and grid tile size (task 0134). */
  readonly viewMode?: 'table' | 'grid';
  readonly iconSize?: GridIconSize;
  readonly onViewModeChange?: (viewMode: 'table' | 'grid', iconSize: GridIconSize) => void;
  readonly columnWidths?: readonly ColumnWidthEntry[] | undefined;
  readonly onColumnWidthChange?: (columnId: string, width: number) => void;
  readonly onFilterQueryChange: (query: string) => void;
  readonly onFilterCommit: () => void;
  readonly onFilterClose: () => void;
  readonly onRename: (entry: EntrySummary, name: string) => void | Promise<void>;
  /** F2 with more than one entry selected opens the multi-rename dialog (task 0072) instead of
   * the single-entry inline rename input. */
  readonly onMultiRename?: (entries: readonly EntrySummary[]) => void;
  readonly onContextMenu?: (entries: readonly EntrySummary[], x: number, y: number) => void;
  readonly onDragStart?: (entries: readonly EntrySummary[], event: DragEvent) => void;
  readonly onDragOver?: (entry: EntrySummary | undefined, event: DragEvent) => boolean;
  readonly onDrop?: (entry: EntrySummary | undefined, event: DragEvent) => void;
  readonly onTabDragOver?: (tabId: TabId, event: DragEvent) => boolean;
  readonly onTabDrop?: (tabId: TabId, event: DragEvent) => void;
  /** When set, replaces the pane's directory-listing surface with this content (task 0088). */
  readonly viewerContent?: m.Children;
  /** Filenames displayed for Lister-owned tabs. */
  readonly viewerTitles?: ReadonlyMap<TabId, string>;
}

/** Inputs for the recursive workspace layout renderer. */
export interface WorkspaceLayoutViewAttrs {
  readonly workspace: WorkspaceProjection;
  readonly paneContent: (paneId: PaneId) => WorkspacePaneContent;
  readonly onActivatePane: (paneId: PaneId) => void;
  readonly onUpdateLayout: (layout: WorkspaceLayout) => void;
  readonly onSelectTab: (paneId: PaneId, tabId: TabId) => void;
  readonly onCloseTab: (paneId: PaneId, tabId: TabId) => void;
  readonly onNewTab: (paneId: PaneId) => void;
  readonly onMoveTab: (
    sourcePaneId: PaneId,
    tabId: TabId,
    targetPaneId: PaneId,
    targetIndex: number,
  ) => void;
  /** Moves focus into the terminal when it is visible for the active folder. */
  readonly onFocusTerminal?: () => boolean;
  /** Moves focus into an open F3 viewer (its find-in-file search input for text content) instead
   * of cycling `activePaneId` - Total Commander's Lister convention. Returns whether a viewer was
   * open and focus was redirected there; when it returns `false` (or is unset), the normal
   * pane-to-pane Tab cycle proceeds. Checked before `onPaneCycleBoundary` so it applies on every
   * Tab press while a viewer is open, not only at the ends of the pane cycle. */
  readonly onFocusViewer?: () => boolean;
  /** Called when Tab/Shift+Tab would otherwise wrap past the last/first pane in the split - lets
   * the caller redirect focus to another UI surface (the directory-tree sidebar, task 0139)
   * instead of wrapping directly back around the pane cycle. Returns whether it redirected
   * focus; when it returns `false` (or is unset), the normal pane-to-pane wrap proceeds. */
  readonly onPaneCycleBoundary?: () => boolean;
  /**
   * Lets the caller force-persist an in-flight debounced layout edit (e.g.
   * before switching workspaces) by handing it a callback registered once on
   * init.
   */
  readonly registerFlush?: (flush: () => void) => void;
  /** Registers a callback (once, on init) the caller can invoke to move DOM focus into a pane -
   * e.g. after a filename search (Alt+F7) navigates a pane to its results and closes the dialog,
   * so arrow-key cursor movement works immediately without an extra click. */
  readonly registerFocusPane?: (focusPane: (paneId: PaneId) => void) => void;
  /** Resolves the user-facing kind and term for a `search://` tab location. */
  readonly searchPresentationForLocationUri?: (uri: string) => SearchPresentation | undefined;
  /** Re-runs the request that produced a `search://` tab. */
  readonly onRefreshSearch?: (uri: string, paneId: PaneId) => void;
}

/** Clamps a horizontal split so both children retain a usable minimum width. */
export function constrainSplitRatio(
  pointerOffset: number,
  containerWidth: number,
  minimumPaneWidth = MIN_PANE_WIDTH,
): number {
  if (containerWidth <= 0) {
    return 0.5;
  }
  const minimumRatio = Math.min(minimumPaneWidth / containerWidth, 0.5);
  return Math.min(1 - minimumRatio, Math.max(minimumRatio, pointerOffset / containerWidth));
}

export function pathFromUri(uri: string): string {
  if (uri.startsWith('archive://')) {
    return decodeURIComponent(uri.slice('archive://'.length)) || '/';
  }
  if (uri.startsWith('file://')) {
    return decodeURIComponent(uri.slice('file://'.length)) || '/';
  }
  if (uri.startsWith('mock:///')) {
    const path = decodeURIComponent(uri.slice('mock://'.length));
    return path.length === 0 ? '/' : path;
  }
  if (/^(sftp|ftp|ftps|onedrive|webdav|s3):\/\//.test(uri)) {
    const withoutScheme = uri.slice(uri.indexOf('://') + 3);
    const slashIndex = withoutScheme.indexOf('/');
    if (slashIndex === -1) {
      return '/';
    }
    const remotePath = decodeURIComponent(withoutScheme.slice(slashIndex));
    return remotePath.length === 0 ? '/' : remotePath;
  }
  return uri;
}

/** A search tab's displayed tooltip path, with its opaque id replaced by its friendly label. */
function searchDisplayPath(uri: string, presentation: SearchPresentation | undefined): string {
  const withoutScheme = uri.slice('search://'.length);
  const separatorIndex = withoutScheme.indexOf('/');
  const providerId = separatorIndex === -1 ? withoutScheme : withoutScheme.slice(0, separatorIndex);
  const searchId = separatorIndex === -1 ? '' : withoutScheme.slice(separatorIndex + 1);
  if (presentation === undefined) return `/search/${providerId}/${searchId}`;
  const prefix = presentation.kind === 'filename' ? 'file' : 'content';
  return `/search/${providerId}/${prefix}: ${presentation.label ?? presentation.term}`;
}

/** Displayed breadcrumb path for any tab location, special-casing `search://` (see
 * {@link searchDisplayPath}) - every other scheme delegates to {@link pathFromUri}. */
function displayPathFromUri(uri: string, presentation: SearchPresentation | undefined): string {
  return uri.startsWith('search://') ? searchDisplayPath(uri, presentation) : pathFromUri(uri);
}

/** Displayed tab title for any tab location, e.g. `search: *.svg` for filename-search results. */
function displayTabTitle(
  uri: string,
  title: string,
  presentation: SearchPresentation | undefined,
): string {
  return uri.startsWith('search://') && presentation !== undefined
    ? `search: ${presentation.label ?? presentation.term}`
    : title;
}

/** Bare (unprefixed) tab title for `search://` tabs - the tab strip shows a search icon instead
 * of the textual `search: ` prefix (task 0089 follow-up), so the visible label only needs the
 * query text; see `PaneTab.isSearchTab` for the icon/prefix decision. */
function bareTabTitle(
  uri: string,
  title: string,
  presentation: SearchPresentation | undefined,
): string {
  return uri.startsWith('search://') && presentation !== undefined
    ? (presentation.label ?? presentation.term)
    : title;
}

function connectionRootTitle(
  location: Location,
  fallback: string,
  connections: readonly Connection[] | undefined,
): string {
  const connection = connectionForLocation(location, connections);
  if (connection === undefined || !isBrowsable(connection)) return fallback;
  return remoteRootLocation(connection).uri === location.uri ? connection.name : fallback;
}

function paneIdsInLayout(layout: WorkspaceLayout): readonly PaneId[] {
  if (layout.type === 'pane') {
    return [layout.paneId];
  }
  return [...paneIdsInLayout(layout.first), ...paneIdsInLayout(layout.second)];
}

/** Renders an arbitrary backend workspace layout tree using pane leaves and split nodes. */
export const WorkspaceLayoutView: FactoryComponent<WorkspaceLayoutViewAttrs> = () => {
  const paneElements = new Map<PaneId, HTMLElement>();
  let displayedLayout: WorkspaceLayout | undefined;
  let sourceLayout: WorkspaceLayout | undefined;
  let persistenceTimer: ReturnType<typeof setTimeout> | undefined;
  let initialFocusFrame: number | undefined;
  let pendingLayoutUpdate: { attrs: WorkspaceLayoutViewAttrs; layout: WorkspaceLayout } | undefined;
  let stopDragging: (() => void) | undefined;
  /** Latest render's attrs, for `registerFocusPane`'s callback (invoked outside any render). */
  let latestAttrs: WorkspaceLayoutViewAttrs | undefined;
  function replaceSplit(
    layout: WorkspaceLayout,
    target: WorkspaceLayout,
    ratio: number,
  ): WorkspaceLayout {
    if (layout === target && layout.type === 'split') {
      return { ...layout, ratio };
    }
    if (layout.type === 'pane') {
      return layout;
    }
    return {
      ...layout,
      first: replaceSplit(layout.first, target, ratio),
      second: replaceSplit(layout.second, target, ratio),
    };
  }

  function scheduleLayoutUpdate(attrs: WorkspaceLayoutViewAttrs, layout: WorkspaceLayout): void {
    if (persistenceTimer !== undefined) {
      clearTimeout(persistenceTimer);
    }
    pendingLayoutUpdate = { attrs, layout };
    persistenceTimer = setTimeout(() => {
      persistenceTimer = undefined;
      pendingLayoutUpdate = undefined;
      attrs.onUpdateLayout(layout);
    }, 500);
  }

  /** Immediately persists a pending debounced layout edit, if any, cancelling its timer. */
  function flushPendingLayoutUpdate(): void {
    if (persistenceTimer === undefined || pendingLayoutUpdate === undefined) {
      return;
    }
    clearTimeout(persistenceTimer);
    persistenceTimer = undefined;
    const { attrs, layout } = pendingLayoutUpdate;
    pendingLayoutUpdate = undefined;
    attrs.onUpdateLayout(layout);
  }

  function beginSplitDrag(
    event: PointerEvent,
    attrs: WorkspaceLayoutViewAttrs,
    split: Extract<WorkspaceLayout, { type: 'split' }>,
  ): void {
    event.preventDefault();
    stopDragging?.();
    const container = (event.currentTarget as HTMLElement).parentElement;
    if (container === null) {
      return;
    }
    const move = (moveEvent: PointerEvent): void => {
      const bounds = container.getBoundingClientRect();
      const horizontal = split.axis === 'horizontal';
      const offset = horizontal ? moveEvent.clientX - bounds.left : moveEvent.clientY - bounds.top;
      const extent = horizontal ? bounds.width : bounds.height;
      const ratio = constrainSplitRatio(offset, extent);
      const nextLayout = replaceSplit(attrs.workspace.layout, split, ratio);
      displayedLayout = nextLayout;
      scheduleLayoutUpdate(attrs, nextLayout);
      m.redraw();
    };
    const end = (): void => stopDragging?.();
    stopDragging = () => {
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', end);
      stopDragging = undefined;
    };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', end);
  }

  function focusAndActivate(attrs: WorkspaceLayoutViewAttrs, paneId: PaneId): void {
    const workspacePane = paneElements.get(paneId);
    const keyboardTarget = workspacePane?.querySelector<HTMLElement>('.fm-pane');
    const target = keyboardTarget ?? workspacePane;
    if (target === undefined) return;
    if (document.activeElement === target) attrs.onActivatePane(paneId);
    else target.focus();
  }

  function renderPane(
    attrs: WorkspaceLayoutViewAttrs,
    paneId: PaneId,
  ): Vnode<unknown, unknown> | undefined {
    const pane = attrs.workspace.panesById[paneId];
    if (pane === undefined) {
      return undefined;
    }
    const tab = pane.tabsById[pane.activeTabId];
    if (tab === undefined) {
      return undefined;
    }
    const content = attrs.paneContent(paneId);
    const tabTitle = connectionRootTitle(tab.location, tab.title, content.connections);
    const active = attrs.workspace.activePaneId === paneId;
    return m(
      '.fm-workspace-pane',
      {
        key: paneId,
        'data-pane-id': paneId,
        'data-active': String(active),
        tabindex: active ? 0 : -1,
        oncreate: ({ dom }) => paneElements.set(paneId, dom as HTMLElement),
        // Guard against removal firing *after* a replacement node's `oncreate` (possible since
        // this vnode has no explicit `key`, so a positional diff can create-then-remove instead
        // of patching in place) - an unconditional delete here would wipe the fresh reference and
        // leave `paneId` permanently unfocusable via click/Tab until a full remount.
        onremove: ({ dom }) => {
          if (paneElements.get(paneId) === dom) paneElements.delete(paneId);
        },
        onfocusin: (event: FocusEvent) => {
          // The pane's keyboard surface is the focus/active-pane authority. Ignore descendant
          // controls here: their click handler activates the pane, avoiding a competing
          // setActivePane request before a tab button dispatches its atomic activateTab command.
          if (
            event.target === event.currentTarget ||
            (event.target instanceof HTMLElement && event.target.classList.contains('fm-pane'))
          ) {
            attrs.onActivatePane(paneId);
          }
        },
        onclick: (event: MouseEvent) => {
          // Clicking an interactive control (e.g. the file viewer's search box) must not steal
          // focus back to the directory table - only activate the pane, keep the DOM focus as-is.
          if (
            event.target instanceof HTMLInputElement ||
            event.target instanceof HTMLTextAreaElement ||
            event.target instanceof HTMLSelectElement ||
            event.target instanceof HTMLButtonElement ||
            (event.target instanceof HTMLElement && event.target.isContentEditable)
          ) {
            attrs.onActivatePane(paneId);
            return;
          }
          focusAndActivate(attrs, paneId);
        },
        onkeydown: (event: KeyboardEvent) => {
          if (
            event.target instanceof HTMLInputElement ||
            event.target instanceof HTMLTextAreaElement ||
            event.target instanceof HTMLSelectElement ||
            (event.target instanceof HTMLElement && event.target.isContentEditable)
          ) {
            return;
          }
          if (
            event.key === 'Tab' &&
            event.shiftKey &&
            !event.ctrlKey &&
            !event.metaKey &&
            !event.altKey &&
            attrs.onFocusTerminal?.() === true
          ) {
            event.preventDefault();
            event.stopPropagation();
            return;
          }
          const actionId = dispatchKeybinding(
            event,
            {
              scope: 'table',
              platform: content.platform,
              runtime: content.keybindingRuntime ?? 'browser',
            },
            content.actions ?? [],
            content.keybindingOverrides ?? {},
          );
          if (actionId !== 'core.switchPane') {
            return;
          }
          event.preventDefault();
          // This layout handler also moves DOM focus. Letting the same Tab reach the document
          // handler would switch workspace.activePaneId a second time, back to the old pane.
          event.stopPropagation();
          if (attrs.onFocusViewer?.() === true) {
            return;
          }
          const paneOrder = paneIdsInLayout(attrs.workspace.layout);
          const currentIndex = paneOrder.indexOf(paneId);
          const direction = event.shiftKey ? -1 : 1;
          const rawNextIndex = currentIndex + direction;
          if (
            (rawNextIndex < 0 || rawNextIndex >= paneOrder.length) &&
            attrs.onPaneCycleBoundary?.() === true
          ) {
            return;
          }
          const nextIndex = (rawNextIndex + paneOrder.length) % paneOrder.length;
          const nextPaneId = paneOrder[nextIndex];
          if (nextPaneId !== undefined) {
            focusAndActivate(attrs, nextPaneId);
          }
        },
      },
      m(Pane, {
        paneId,
        path: pathFromUri(tab.location.uri),
        locationUri: tab.location.uri,
        tabTitle:
          content.viewerTitles?.get(tab.id) ??
          displayTabTitle(
            tab.location.uri,
            connectionRootTitle(tab.location, tabTitle, content.connections),
            attrs.searchPresentationForLocationUri?.(tab.location.uri),
          ),
        ...(tab.location.uri.startsWith('search://')
          ? (() => {
              const presentation = attrs.searchPresentationForLocationUri?.(tab.location.uri);
              return presentation === undefined ? {} : { searchPresentation: presentation };
            })()
          : {}),
        ...(tab.location.uri.startsWith('search://') && attrs.onRefreshSearch !== undefined
          ? { onRefreshSearch: () => attrs.onRefreshSearch?.(tab.location.uri, paneId) }
          : {}),
        tabs: pane.tabOrder.map((tabId) => {
          const paneTab = pane.tabsById[tabId];
          const uri = paneTab?.location.uri;
          const connection =
            paneTab === undefined
              ? undefined
              : connectionForLocation(paneTab.location, content.connections);
          const presentation =
            uri === undefined ? undefined : attrs.searchPresentationForLocationUri?.(uri);
          return {
            id: tabId,
            title:
              content.viewerTitles?.get(tabId) !== undefined
                ? (content.viewerTitles.get(tabId) ?? '')
                : paneTab === undefined
                  ? ''
                  : bareTabTitle(
                      uri ?? '',
                      connectionRootTitle(paneTab.location, paneTab.title, content.connections),
                      presentation,
                    ),
            path:
              paneTab === undefined ? '' : displayPathFromUri(paneTab.location.uri, presentation),
            ...(uri === undefined ? {} : { locationUri: uri }),
            isSearchTab: uri?.startsWith('search://') ?? false,
            isConnectionTab: connection !== undefined,
            ...(presentation === undefined ? {} : { searchKind: presentation.kind }),
          };
        }),
        activeTabId: pane.activeTabId,
        onSelectTab: (tabId) => attrs.onSelectTab(paneId, tabId),
        onCloseTab: (tabId) => attrs.onCloseTab(paneId, tabId),
        onNewTab: () => attrs.onNewTab(paneId),
        onMoveTab: attrs.onMoveTab,
        ...(content.onTabDragOver === undefined ? {} : { onTabDragOver: content.onTabDragOver }),
        ...(content.onTabDrop === undefined ? {} : { onTabDrop: content.onTabDrop }),
        favourites: {
          location: content.location,
          defaultLabel: content.defaultFavouriteLabel,
          currentLocationIsSavedSearch: content.currentLocationIsSavedSearch,
          favouriteLocations: content.favouriteLocations,
          recentLocations: content.recentLocations,
          savedSearches: content.savedSearches,
          systemLocations: content.systemLocations,
          systemLocationsError: content.systemLocationsError,
          onRetrySystemLocations: content.onRetrySystemLocations,
          volumes: content.volumes,
          volumesError: content.volumesError,
          onRetryVolumes: content.onRetryVolumes,
          connections: content.connections,
          onManageConnections: content.onManageConnections,
          onOpenSavedSearch: content.onOpenSavedSearch,
          onRefreshConnections: content.onRefreshConnections,
          unavailableLocations: content.unavailableLocations,
          onNavigateLocation: content.onNavigateLocation,
          onAddFavourite: content.onAddFavourite,
          onDeleteFavourite: content.onDeleteFavourite,
          onReorderFavourites: content.onReorderFavourites,
        } satisfies FavouritesAttrs,
        tableConfig: {
          sortLabel: content.sortLabel,
          sort: content.sort,
          formatSettings: content.formatSettings,
          pluginColumns: content.pluginColumns,
          visibleColumnIds: content.visibleColumnIds,
          showGitStatusColumn: content.showGitStatusColumn,
          nativeIconLoader: content.nativeIconLoader,
          thumbnailLoader: content.thumbnailLoader,
          finderTagsLoader: content.finderTagsLoader,
          viewMode: content.viewMode,
          iconSize: content.iconSize,
          onViewModeChange: content.onViewModeChange,
          columnWidths: content.columnWidths,
          onColumnWidthChange: content.onColumnWidthChange,
        } satisfies TableConfigAttrs,
        directorySummary: {
          hasMore: content.hasMore,
          totalEntryCount: content.totalEntryCount,
          totalKnownEntries: content.totalKnownEntries,
          totalKnownSize: content.totalKnownSize,
          totalKnownFileCount: content.totalKnownFileCount,
          volumeCapacity: content.volumeCapacity,
          hiddenSelectedCount: content.hiddenSelectedCount,
        } satisfies DirectorySummaryAttrs,
        filter: {
          filterOpen: content.filterOpen,
          filterQuery: content.filterQuery,
          onFilterQueryChange: content.onFilterQueryChange,
          onFilterCommit: content.onFilterCommit,
          onFilterClose: content.onFilterClose,
        } satisfies FilterAttrs,
        navigation: {
          onNavigate: content.onNavigate,
          onBack: content.onBack,
          onForward: content.onForward,
          onParent: content.onParent,
          canNavigateBack: tab.canNavigateBack,
          canNavigateForward: tab.canNavigateForward,
        } satisfies PaneNavigationAttrs,
        state: content.state,
        entries: content.entries,
        selectedEntryIds: content.selectedEntryIds,
        cutEntryIds: content.cutEntryIds,
        active,
        platform: content.platform,
        ...(content.keybindingRuntime === undefined
          ? {}
          : { keybindingRuntime: content.keybindingRuntime }),
        ...(content.actions === undefined ? {} : { actions: content.actions }),
        ...(content.keybindingOverrides === undefined
          ? {}
          : { keybindingOverrides: content.keybindingOverrides }),
        ...(content.cursorIndex === undefined ? {} : { cursorIndex: content.cursorIndex }),
        onOpenEntry: content.onOpenEntry,
        onSelectionAction: content.onSelectionAction,
        onRetry: content.onRetry,
        onLoadNextPage: content.onLoadNextPage,
        onSortChange: content.onSortChange,
        onRename: content.onRename,
        ...(content.onMultiRename === undefined ? {} : { onMultiRename: content.onMultiRename }),
        onContextMenu: content.onContextMenu ?? (() => undefined),
        ...(content.onDragStart === undefined ? {} : { onDragStart: content.onDragStart }),
        ...(content.onDragOver === undefined ? {} : { onDragOver: content.onDragOver }),
        ...(content.onDrop === undefined ? {} : { onDrop: content.onDrop }),
        ...(content.viewerContent === undefined ? {} : { viewerContent: content.viewerContent }),
      }),
    );
  }

  function renderLayout(
    attrs: WorkspaceLayoutViewAttrs,
    layout: WorkspaceLayout,
    path = 'root',
  ): Vnode<unknown, unknown> | undefined {
    if (layout.type === 'pane') {
      return renderPane(attrs, layout.paneId);
    }
    return m(
      '.fm-workspace-split',
      {
        // `renderPane`'s pane vnode below is keyed by `paneId` (Mithril requires every vnode in a
        // children array to be keyed, or none), so a split's own container and splitter need a
        // key too - `path` (this node's position in the layout tree) is stable across re-renders
        // for the same reason positional diffing already relied on tree shape being stable, and
        // it protects the split/pane DOM nodes (and the focus living inside them) from being torn
        // down and rebuilt merely because a workspace refetch produced new object references for
        // unchanged panes (e.g. the cross-window snapshot merge in app-shell's
        // `applyRemoteWorkspaceSnapshot`).
        key: path,
        class: `fm-workspace-split--${layout.axis}`,
        style:
          layout.axis === 'horizontal'
            ? { gridTemplateColumns: `${layout.ratio}fr auto ${1 - layout.ratio}fr` }
            : { gridTemplateRows: `${layout.ratio}fr auto ${1 - layout.ratio}fr` },
      },
      [
        renderLayout(attrs, layout.first, `${path}.first`),
        m('.fm-workspace-splitter', {
          key: `${path}.splitter`,
          role: 'separator',
          'aria-orientation': layout.axis === 'horizontal' ? 'vertical' : 'horizontal',
          tabindex: 0,
          onpointerdown: (event: PointerEvent) => beginSplitDrag(event, attrs, layout),
        }),
        renderLayout(attrs, layout.second, `${path}.second`),
      ],
    );
  }

  return {
    oninit: ({ attrs }) => {
      attrs.registerFlush?.(flushPendingLayoutUpdate);
      attrs.registerFocusPane?.((paneId) => {
        if (latestAttrs !== undefined) focusAndActivate(latestAttrs, paneId);
      });
    },
    oncreate: ({ attrs }) => {
      initialFocusFrame = requestAnimationFrame(() => {
        initialFocusFrame = undefined;
        if (document.activeElement === document.body) {
          focusAndActivate(attrs, attrs.workspace.activePaneId);
        }
      });
    },
    onremove: () => {
      stopDragging?.();
      if (initialFocusFrame !== undefined) {
        cancelAnimationFrame(initialFocusFrame);
      }
      if (persistenceTimer !== undefined) {
        clearTimeout(persistenceTimer);
      }
    },
    view: ({ attrs }) => {
      latestAttrs = attrs;
      if (sourceLayout !== attrs.workspace.layout) {
        sourceLayout = attrs.workspace.layout;
        displayedLayout = sourceLayout;
      }
      return m('.fm-workspace-layout', { 'aria-label': `${attrs.workspace.name} workspace` }, [
        renderLayout(attrs, displayedLayout ?? attrs.workspace.layout),
      ]);
    },
  };
};
