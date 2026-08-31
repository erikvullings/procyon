import type {
  ActionDescriptor,
  Connection,
  Location,
  PaneId,
  SortDescriptor,
  SystemLocation,
  Volume,
  WorkspaceId,
} from '../../models';
import { isBrowsable, remoteRootLocation } from '../connections/connections-model';
import { SORT_SHORTCUT_DESCRIPTORS } from '../keybindings/global-keydown-handler';

/** Frontend-local id for the App menu's Preferences item; never an action-registry id (there is
 * no `core.preferences` backend action - Preferences is, and must remain, a pure UI toggle). */
export const OPEN_SETTINGS_MENU_ID = 'ui.openSettings';
export const OPEN_DIAGNOSTICS_MENU_ID = 'ui.openDiagnostics';
export const OPEN_SHORTCUTS_MENU_ID = 'core.showShortcutsHelp';

/** Prefix for a Window-menu tab item's id, followed by the tab's `${paneId}:${tabId}` key. */
export const WINDOW_TAB_MENU_ID_PREFIX = 'ui.window.tab.';

/** File menu's "New Window" item; never an action-registry id, like `OPEN_SETTINGS_MENU_ID` -
 * opening a workspace in a new OS window (task 0143) is desktop-window plumbing, not a backend
 * action. */
export const NEW_WORKSPACE_WINDOW_MENU_ID = 'ui.newWorkspaceWindow';

/** File menu's "Sync Workspace" item; never an action-registry id, like
 * `NEW_WORKSPACE_WINDOW_MENU_ID` - writing this window's ephemeral workspace back into its named
 * source (ephemeral per-window workspaces spec follow-up) is desktop-window plumbing, not a
 * backend action. */
export const SYNC_WORKSPACE_MENU_ID = 'ui.syncWorkspace';

/** File-menu tab actions are frontend-owned, despite being registered in the shared action list. */
export const NEW_TAB_MENU_ID = 'core.newTab';
export const CLOSE_TAB_MENU_ID = 'core.closeTab';

/** Prefix for the Window menu's "Open Workspace" submenu item ids, followed by the target
 * workspace's id - opens that workspace in its own OS window, mirroring the workspace switcher's
 * "open in new window" button (task 0143 follow-up). */
export const WINDOW_OPEN_WORKSPACE_MENU_ID_PREFIX = 'ui.window.openWorkspace.';

/** Prefix for a Go-menu Volumes-group item id, followed by the item's index into the `volumes`
 * array passed to `native-menu-spec.ts`'s `NativeMenuInputs` (task 0144), mirroring
 * `core.favourite.<index>`. */
export const GO_MENU_VOLUME_ID_PREFIX = 'ui.goMenu.volume.';

/** Prefix for a Go-menu Servers-group item id, followed by the target connection's id (task
 * 0144). */
export const GO_MENU_CONNECTION_ID_PREFIX = 'ui.goMenu.connection.';

/** Prefix for a Go-menu Cloud/Network-group item id, followed by the item's index into the
 * `systemLocations` array passed to `NativeMenuInputs` (task 0144). Cloud and Network share one
 * prefix because they share one source array, indexed the same way the dropdown's Cloud/Network
 * sections filter it. */
export const GO_MENU_SYSTEM_LOCATION_ID_PREFIX = 'ui.goMenu.systemLocation.';

/** Context the click router needs; kept minimal so it's independently testable from app-shell.ts's
 * closures rather than left inline and untestable. */
export interface NativeMenuDispatchContext {
  /** Looked up by id for anything that isn't a frontend-local menu id - typically
   * `actionsWithFavourites()`, since the Go menu's favourites are synthetic entries not present in
   * the plain action registry. */
  readonly findAction: (id: string) => ActionDescriptor | undefined;
  readonly openSettingsDialog: () => void;
  readonly openDiagnostics: () => void;
  readonly openShortcutsHelp: () => void;
  /** Activates the tab encoded in a `ui.window.tab.<tabKey>` id (the `${paneId}:${tabId}` key
   * app-shell.ts's tab caches are keyed by). */
  readonly activateTabByKey: (tabKey: string) => void;
  readonly openNewTab: (paneId: PaneId) => void;
  readonly closeActiveTab: (paneId: PaneId) => void;
  /** The pane the sort-menu items apply to - same "active pane" concept the Ctrl+F3..Ctrl+F7
   * shortcuts use (`activeDirectory()`'s `paneId` in app-shell.ts). */
  readonly activePaneId: () => PaneId | undefined;
  /** Applies a sort-menu item's fixed sort to a pane - the same local view-state update the
   * Ctrl+F3..Ctrl+F7 shortcuts make (`GlobalKeydownContext.setSort`), not a backend action
   * dispatch: `core.sortByName` etc. have no backend effect to invoke, exactly like
   * `core.preferences` has none - sorting is frontend-owned workspace view state. */
  readonly setSort: (paneId: PaneId, sort: readonly SortDescriptor[]) => void;
  /** The single dispatch function already used by the command palette/context menu
   * (`action-command-controller.ts`'s `invokePaletteAction`) - reused here rather than duplicated
   * or bypassed, so its `core.favourites`/`core.favourite.N`/`core.createDirectory`/clipboard
   * special-casing stays in exactly one place. */
  readonly invokeAction: (action: ActionDescriptor) => void;
  /** Opens the current workspace in a new OS window (task 0143); absent on hosts with no window
   * concept, in which case `NEW_WORKSPACE_WINDOW_MENU_ID` clicks are silently ignored - the item
   * is never added to the menu spec on those hosts in the first place (see
   * `native-menu-spec.ts`'s `NativeMenuInputs.canOpenNewWindow`), so this should be unreachable in
   * practice. */
  readonly openNewWorkspaceWindow?: () => void;
  /** Opens the given workspace (any workspace, not just the current one) in its own OS window -
   * backs the Window menu's "Open Workspace" submenu. Same desktop-only absence rule as
   * `openNewWorkspaceWindow`. */
  readonly openWorkspaceWindowById?: (workspaceId: WorkspaceId) => void;
  /** Writes this window's ephemeral workspace back into the named workspace it was forked from
   * (ephemeral per-window workspaces spec follow-up) - backs the File menu's "Sync Workspace"
   * item. Same desktop-only absence rule as `openNewWorkspaceWindow`. */
  readonly resyncWorkspace?: () => void;
  /** The same `volumes`/`connections`/`systemLocations` arrays passed to
   * `native-menu-spec.ts`'s `NativeMenuInputs` (task 0144), so a Go-menu click's synthetic id can
   * be resolved back to the location it names. Read live via getters, like `activePaneId` above,
   * since this context object is built once while the underlying arrays are reassigned as they
   * reload. */
  readonly getVolumes: () => readonly Volume[];
  readonly getConnections: () => readonly Connection[];
  readonly getSystemLocations: () => readonly SystemLocation[];
  /** Navigates the active pane to `location` - the same navigation path `pane.ts`'s
   * `navigateFavourite` uses (task 0144), shared here rather than duplicated. */
  readonly navigateToLocation: (location: Location) => void;
}

/**
 * Routes one `{ id }` click received from the native menu bar's `subscribe_native_menu_actions`
 * channel. A stale id from a menu the backend hasn't rebuilt yet is a silent no-op, never a throw.
 */
export function dispatchNativeMenuAction(context: NativeMenuDispatchContext, id: string): void {
  if (id === OPEN_SETTINGS_MENU_ID) {
    context.openSettingsDialog();
    return;
  }
  if (id === OPEN_DIAGNOSTICS_MENU_ID) {
    context.openDiagnostics();
    return;
  }
  if (id === OPEN_SHORTCUTS_MENU_ID) {
    context.openShortcutsHelp();
    return;
  }
  if (id.startsWith(WINDOW_TAB_MENU_ID_PREFIX)) {
    context.activateTabByKey(id.slice(WINDOW_TAB_MENU_ID_PREFIX.length));
    return;
  }
  if (id === NEW_WORKSPACE_WINDOW_MENU_ID) {
    context.openNewWorkspaceWindow?.();
    return;
  }
  if (id === SYNC_WORKSPACE_MENU_ID) {
    context.resyncWorkspace?.();
    return;
  }
  if (id === NEW_TAB_MENU_ID || id === CLOSE_TAB_MENU_ID) {
    const paneId = context.activePaneId();
    if (paneId === undefined) return;
    if (id === NEW_TAB_MENU_ID) context.openNewTab(paneId);
    else context.closeActiveTab(paneId);
    return;
  }
  if (id.startsWith(WINDOW_OPEN_WORKSPACE_MENU_ID_PREFIX)) {
    context.openWorkspaceWindowById?.(
      id.slice(WINDOW_OPEN_WORKSPACE_MENU_ID_PREFIX.length) as WorkspaceId,
    );
    return;
  }
  if (id.startsWith(GO_MENU_VOLUME_ID_PREFIX)) {
    const volume = context.getVolumes()[Number(id.slice(GO_MENU_VOLUME_ID_PREFIX.length))];
    if (volume !== undefined) context.navigateToLocation(volume.location);
    return;
  }
  if (id.startsWith(GO_MENU_CONNECTION_ID_PREFIX)) {
    const connectionId = id.slice(GO_MENU_CONNECTION_ID_PREFIX.length);
    const connection = context.getConnections().find((candidate) => candidate.id === connectionId);
    if (connection !== undefined && isBrowsable(connection)) {
      context.navigateToLocation(remoteRootLocation(connection));
    }
    return;
  }
  if (id.startsWith(GO_MENU_SYSTEM_LOCATION_ID_PREFIX)) {
    const systemLocation =
      context.getSystemLocations()[Number(id.slice(GO_MENU_SYSTEM_LOCATION_ID_PREFIX.length))];
    if (systemLocation !== undefined) context.navigateToLocation(systemLocation.location);
    return;
  }
  if (id in SORT_SHORTCUT_DESCRIPTORS) {
    const paneId = context.activePaneId();
    if (paneId === undefined) return;
    context.setSort(paneId, SORT_SHORTCUT_DESCRIPTORS[id] ?? []);
    return;
  }
  const action = context.findAction(id);
  if (action === undefined) return;
  context.invokeAction(action);
}
