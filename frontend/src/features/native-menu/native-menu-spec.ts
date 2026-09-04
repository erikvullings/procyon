import { actionTitle, t } from '../../i18n';
import type {
  ActionDescriptor,
  Connection,
  Location,
  NativeMenu,
  NativeMenuItem,
  NativeMenuSpec,
  PaneId,
  SystemLocation,
  TabId,
  Volume,
  WorkspaceId,
  WorkspaceSummary,
} from '../../models';
import { connectionStatusLabel, isBrowsable } from '../connections/connections-model';
import {
  NEW_WORKSPACE_WINDOW_MENU_ID,
  SYNC_WORKSPACE_MENU_ID,
  WINDOW_OPEN_WORKSPACE_MENU_ID_PREFIX,
} from './native-menu-dispatch';

function locationKey(location: Location): string {
  return `${location.providerId}:${location.uri}`;
}

/** One open tab, flattened for the Window menu (task 0133). */
export interface NativeMenuTab {
  readonly paneId: PaneId;
  readonly tabId: TabId;
  /** The composite `${paneId}:${tabId}` key app-shell.ts already keys its per-tab caches by
   * (see its `tabKey` helper) - reused here so a menu click can be routed back to the exact tab
   * without re-deriving the encoding. */
  readonly tabKey: string;
  readonly title: string;
  readonly active: boolean;
}

/** Inputs the pure spec-builder needs; all sourced from app-shell.ts's own closures. */
export interface NativeMenuInputs {
  /** `registeredActions` - File/Edit/View/Help items are looked up from here. */
  readonly actions: readonly ActionDescriptor[];
  /** `favouriteActions()`'s output (`core.favourites` plus one `core.favourite.<index>` per
   * saved location) - the Go menu is built from this alone, not `actions`. */
  readonly favouriteActions: readonly ActionDescriptor[];
  /** Every open tab across every pane, in display order. */
  readonly tabs: readonly NativeMenuTab[];
  /** Whether the host can open a workspace in its own OS window (task 0143) - `false` on hosts
   * with no window concept (browser/HTTP), in which case the File menu's "New Window" item and
   * the Window menu's "Open Workspace" submenu are omitted entirely rather than added disabled. */
  readonly canOpenNewWindow: boolean;
  /** Whether Edit-menu text commands should use the OS responder chain (required by WKWebView). */
  readonly useNativeEditRoles: boolean;
  /** Every stored workspace, in display order - backs the Window menu's "Open Workspace"
   * submenu (task 0143 follow-up), the native-menu equivalent of the workspace switcher's list. */
  readonly workspaces: readonly WorkspaceSummary[];
  /** The workspace currently shown in this window, if any - the "Open Workspace" submenu checks
   * its matching entry, mirroring the switcher's active-workspace highlight. */
  readonly currentWorkspaceId: WorkspaceId | undefined;
  /** Currently mounted local/removable/disk-image volumes (task 0144) - the Go menu's `VOLUMES`
   * group, same data the favourites dropdown's Volumes section consumes. */
  readonly volumes: readonly Volume[];
  /** Saved application-managed connections - the Go menu's `SERVERS` group, mirroring the
   * favourites dropdown's Servers section. */
  readonly connections: readonly Connection[];
  /** OS-discovered locations - the Go menu's `CLOUD`/`NETWORK` groups, mirroring the favourites
   * dropdown's Cloud/Network sections. */
  readonly systemLocations: readonly SystemLocation[];
  /** Location keys (`locationKey`-encoded) that recently failed to navigate - labels the
   * matching Volumes/Cloud/Network item `(unavailable)`, mirroring the favourites dropdown. */
  readonly unavailableLocations: ReadonlySet<string>;
}

const PREFERENCES_SHORTCUT = { key: ',', meta: true } as const;

function findAction(
  actions: readonly ActionDescriptor[],
  id: string,
): ActionDescriptor | undefined {
  return actions.find((action) => action.id === id);
}

function actionItem(action: ActionDescriptor): NativeMenuItem {
  const shortcut = action.defaultShortcuts[0];
  return {
    kind: 'action',
    id: action.id,
    title: actionTitle(action.id, action.title),
    ...(shortcut === undefined ? {} : { shortcut }),
    enabled: true,
    checked: false,
  };
}

/** Looks up each id in `actions`; ids that aren't currently registered (capabilities/plugins can
 * change what's registered) are silently skipped rather than crashing the whole menu. */
function actionItems(
  actions: readonly ActionDescriptor[],
  ids: readonly string[],
): NativeMenuItem[] {
  const items: NativeMenuItem[] = [];
  for (const id of ids) {
    const action = findAction(actions, id);
    if (action !== undefined) items.push(actionItem(action));
  }
  return items;
}

function appMenu(): NativeMenu {
  return {
    // The platform adapter also uses this as the process's displayed name (task 0133 follow-up),
    // so AppKit's bold app-menu title reads "Procyon" even in an unbundled `cargo tauri dev` run,
    // matching the title bar label elsewhere in this file.
    title: t('menu', 'app'),
    items: [
      { kind: 'role', role: 'about' },
      { kind: 'separator' },
      {
        kind: 'action',
        id: 'ui.openSettings',
        title: t('menu', 'preferences'),
        shortcut: PREFERENCES_SHORTCUT,
        enabled: true,
        checked: false,
      },
      { kind: 'separator' },
      { kind: 'role', role: 'services' },
      { kind: 'separator' },
      { kind: 'role', role: 'hideApp' },
      { kind: 'role', role: 'hideOthers' },
      { kind: 'role', role: 'showAll' },
      { kind: 'separator' },
      { kind: 'role', role: 'quit' },
    ],
  };
}

function fileMenu(actions: readonly ActionDescriptor[], canOpenNewWindow: boolean): NativeMenu {
  const newWindowItem: NativeMenuItem = {
    kind: 'action',
    id: NEW_WORKSPACE_WINDOW_MENU_ID,
    title: t('menu', 'newWindow'),
    shortcut: { key: 'n', meta: true, shift: true },
    enabled: true,
    checked: false,
  };
  // No default shortcut, like `newWindowItem` itself has none of its own beyond the one above.
  const syncWorkspaceItem: NativeMenuItem = {
    kind: 'action',
    id: SYNC_WORKSPACE_MENU_ID,
    title: t('menu', 'syncWorkspace'),
    enabled: true,
    checked: false,
  };
  return {
    title: t('menu', 'file'),
    items: [
      ...(canOpenNewWindow ? [newWindowItem, syncWorkspaceItem] : []),
      ...actionItems(actions, ['core.newTab', 'core.closeTab']),
    ],
  };
}

/** Only Copy/Paste/Select All: this app has no Undo/Redo feature and no Cut action anywhere in
 * the registry. Native AppKit already gives Cut/Copy/Paste/Undo inside text fields (e.g. the
 * Preferences dialog) for free via the standard responder chain - no menu wiring needed there. */
function editMenu(actions: readonly ActionDescriptor[], useNativeEditRoles: boolean): NativeMenu {
  return {
    title: t('menu', 'edit'),
    items: useNativeEditRoles
      ? [
          { kind: 'role', role: 'copy' },
          { kind: 'role', role: 'paste' },
          { kind: 'role', role: 'selectAll' },
        ]
      : actionItems(actions, ['core.copy', 'core.paste', 'core.selectAll']),
  };
}

/** The registry has no dedicated "view" category; the closest genuinely view-related actions are
 * the sort-order toggles (categorized "navigation" on the backend, but they change how the
 * listing displays, not where it navigates). */
function viewMenu(actions: readonly ActionDescriptor[]): NativeMenu {
  return {
    title: t('menu', 'view'),
    items: actionItems(actions, [
      'core.sortByName',
      'core.sortByExtension',
      'core.sortByDate',
      'core.sortBySize',
      'core.sortUnsorted',
    ]),
  };
}

/** Miscellaneous per-selection/location utilities - the same ids the "tools" category groups in
 * the command palette and context menu (`action.rs`'s `core_action(..., "tools", ...)` /
 * `"clipboard"` categories), surfaced as their own top-level menu for discoverability. */
function toolsMenu(actions: readonly ActionDescriptor[]): NativeMenu {
  return {
    title: t('menu', 'tools'),
    items: [
      {
        kind: 'action',
        id: 'ui.openSettings',
        title: t('menu', 'preferences'),
        shortcut: PREFERENCES_SHORTCUT,
        enabled: true,
        checked: false,
      },
      { kind: 'separator' },
      ...actionItems(actions, [
        'core.copyName',
        'core.copyPath',
        'core.copyRelativePath',
        'core.openTerminal',
        'core.revealInSystemFileManager',
      ]),
    ],
  };
}

/** Excludes `core.favourites` itself: invoking it opens the command palette pre-filtered to
 * favourites (its intended behaviour from the palette/keyboard), which makes no sense as a native
 * menu item - the Go menu already lists each saved favourite as its own `core.favourite.<index>`
 * item below, so it's the menu itself acting as the favourites browser, not a launcher for one.
 *
 * After the favourites, mirrors the favourites dropdown's remaining sections in the same order
 * (Volumes, Servers, Cloud, Network - Recent is intentionally omitted, it isn't in the dropdown
 * order relative to these either): each group gets its own leading separator, including the first
 * one, so it reads as visually distinct from the plain favourite items above it. */
function goMenu(
  favouriteActions: readonly ActionDescriptor[],
  volumes: readonly Volume[],
  connections: readonly Connection[],
  systemLocations: readonly SystemLocation[],
  unavailableLocations: ReadonlySet<string>,
): NativeMenu {
  const items: NativeMenuItem[] = favouriteActions
    .filter((action) => action.id !== 'core.favourites')
    .map(actionItem);

  if (volumes.length > 0) {
    items.push({ kind: 'separator' });
    volumes.forEach((volume, index) => {
      const unavailable = unavailableLocations.has(locationKey(volume.location));
      items.push({
        kind: 'action',
        id: `ui.goMenu.volume.${index}`,
        title: unavailable ? `${volume.name} (${t('menu', 'unavailable')})` : volume.name,
        enabled: true,
        checked: false,
      });
    });
  }

  const serverConnections = connections.filter(({ kind }) => kind !== 'oneDrive');
  if (serverConnections.length > 0) {
    items.push({ kind: 'separator' });
    serverConnections.forEach((connection) => {
      // Matches the dropdown's `disabled: !isBrowsable(connection)` (browsable connections stay
      // clickable regardless of status - clicking one that isn't connected yet still navigates
      // and connects). The dropdown additionally shows a status glyph next to every connection;
      // a native menu item has no glyph, so the status is folded into the title text instead,
      // suppressed only for the common "nothing to say" case of an already-connected server.
      const browsable = isBrowsable(connection);
      const title =
        connection.status === 'connected'
          ? connection.name
          : `${connection.name} (${connectionStatusLabel(connection.status)})`;
      items.push({
        kind: 'action',
        id: `ui.goMenu.connection.${connection.id}`,
        title,
        enabled: browsable,
        checked: false,
      });
    });
  }

  const oneDriveConnections = connections.filter(
    (connection) => connection.kind === 'oneDrive' && isBrowsable(connection),
  );
  const cloudLocations = systemLocations.filter(({ kind }) => kind === 'cloud');
  if (oneDriveConnections.length > 0 || cloudLocations.length > 0) {
    items.push({ kind: 'separator' });
    for (const connection of oneDriveConnections) {
      const title =
        connection.status === 'connected'
          ? connection.name
          : `${connection.name} (${connectionStatusLabel(connection.status)})`;
      items.push({
        kind: 'action',
        id: `ui.goMenu.connection.${connection.id}`,
        title,
        enabled: true,
        checked: false,
      });
    }
    for (const systemLocation of cloudLocations) {
      const unavailable = unavailableLocations.has(locationKey(systemLocation.location));
      items.push({
        kind: 'action',
        id: `ui.goMenu.systemLocation.${systemLocations.indexOf(systemLocation)}`,
        title: unavailable
          ? `${systemLocation.name} (${t('menu', 'unavailable')})`
          : systemLocation.name,
        enabled: true,
        checked: false,
      });
    }
  }

  const networkLocations = systemLocations.filter(({ kind }) => kind === 'network');
  if (networkLocations.length > 0) {
    items.push({ kind: 'separator' });
    for (const systemLocation of networkLocations) {
      const unavailable = unavailableLocations.has(locationKey(systemLocation.location));
      items.push({
        kind: 'action',
        id: `ui.goMenu.systemLocation.${systemLocations.indexOf(systemLocation)}`,
        title: unavailable
          ? `${systemLocation.name} (${t('menu', 'unavailable')})`
          : systemLocation.readOnly === true
            ? `${systemLocation.name} (${t('menu', 'readOnly')})`
            : systemLocation.name,
        enabled: true,
        checked: false,
      });
    }
  }

  return { title: t('menu', 'go'), items };
}

function windowMenu(
  tabs: readonly NativeMenuTab[],
  canOpenNewWindow: boolean,
  workspaces: readonly WorkspaceSummary[],
  currentWorkspaceId: WorkspaceId | undefined,
): NativeMenu {
  const items: NativeMenuItem[] = [
    { kind: 'role', role: 'minimize' },
    { kind: 'role', role: 'zoom' },
  ];
  if (canOpenNewWindow && workspaces.length > 0) {
    items.push({ kind: 'separator' });
    items.push({
      kind: 'submenu',
      title: t('menu', 'openWorkspace'),
      items: workspaces.map((workspace) => ({
        kind: 'action',
        id: `${WINDOW_OPEN_WORKSPACE_MENU_ID_PREFIX}${workspace.id}`,
        title: workspace.name,
        enabled: true,
        checked: workspace.id === currentWorkspaceId,
      })),
    });
  }
  if (tabs.length > 0) {
    items.push({ kind: 'separator' });
    for (const tab of tabs) {
      items.push({
        kind: 'action',
        id: `ui.window.tab.${tab.tabKey}`,
        title: tab.title,
        enabled: true,
        checked: tab.active,
      });
    }
  }
  return { title: t('menu', 'window'), items };
}

function helpMenu(actions: readonly ActionDescriptor[]): NativeMenu {
  return {
    title: t('menu', 'help'),
    items: [
      { kind: 'role', role: 'about' },
      {
        kind: 'action',
        id: 'ui.openDiagnostics',
        title: t('shell', 'diagnostics'),
        enabled: true,
        checked: false,
      },
      { kind: 'separator' },
      ...actionItems(actions, ['core.showShortcutsHelp']),
    ],
  };
}

/**
 * Pure function computing the full native menu bar spec from app-shell.ts's own state. No I/O:
 * the caller diffs the result against what it last pushed and only then calls
 * `invoke('set_native_menu', ...)` (see `syncNativeMenu` in app-shell.ts).
 */
export function buildNativeMenuSpec(inputs: NativeMenuInputs): NativeMenuSpec {
  return {
    menus: [
      appMenu(),
      fileMenu(inputs.actions, inputs.canOpenNewWindow),
      editMenu(inputs.actions, inputs.useNativeEditRoles),
      viewMenu(inputs.actions),
      toolsMenu(inputs.actions),
      goMenu(
        inputs.favouriteActions,
        inputs.volumes,
        inputs.connections,
        inputs.systemLocations,
        inputs.unavailableLocations,
      ),
      windowMenu(
        inputs.tabs,
        inputs.canOpenNewWindow,
        inputs.workspaces,
        inputs.currentWorkspaceId,
      ),
      helpMenu(inputs.actions),
    ],
  };
}
