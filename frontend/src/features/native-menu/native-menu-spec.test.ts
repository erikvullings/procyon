import { describe, expect, it } from 'vitest';

import type {
  ActionDescriptor,
  Connection,
  KeyChord,
  SystemLocation,
  Volume,
  WorkspaceSummary,
} from '../../models';
import { buildNativeMenuSpec, type NativeMenuInputs, type NativeMenuTab } from './native-menu-spec';

function action(
  id: string,
  title: string,
  defaultShortcuts: KeyChord[] = [],
  category = 'test',
): ActionDescriptor {
  return {
    id,
    title,
    category,
    defaultShortcuts,
    contextRequirements: {},
    source: { kind: 'core' },
  };
}

function tab(overrides: Partial<NativeMenuTab> = {}): NativeMenuTab {
  return {
    paneId: 'pane-1',
    tabId: 'tab-1',
    tabKey: 'pane-1:tab-1',
    title: 'Documents',
    active: false,
    ...overrides,
  };
}

function workspaceSummary(overrides: Partial<WorkspaceSummary> = {}): WorkspaceSummary {
  return {
    id: 'workspace-1',
    name: 'Default',
    revision: 0,
    ephemeral: false,
    updatedAt: '2026-01-01T00:00:00Z',
    ...overrides,
  };
}

function inputs(overrides: Partial<NativeMenuInputs> = {}): NativeMenuInputs {
  return {
    actions: [],
    favouriteActions: [],
    tabs: [],
    canOpenNewWindow: false,
    workspaces: [],
    currentWorkspaceId: undefined,
    volumes: [],
    connections: [],
    systemLocations: [],
    unavailableLocations: new Set(),
    ...overrides,
  };
}

function connection(overrides: Partial<Connection> = {}): Connection {
  return {
    id: 'connection-1',
    name: 'Home Server',
    kind: 'ssh',
    configuration: {
      kind: 'ssh',
      host: 'example.test',
      port: 22,
      username: 'erik',
      startPath: null,
      authentication: 'password',
      hostKeyPolicy: 'promptOnFirstUse',
      keepaliveSeconds: null,
    },
    hasCredential: true,
    status: 'disconnected',
    createdAt: '2026-01-01T00:00:00.000Z',
    updatedAt: '2026-01-01T00:00:00.000Z',
    ...overrides,
  };
}

describe('buildNativeMenuSpec', () => {
  it('emits every top-level menu in order', () => {
    const spec = buildNativeMenuSpec(inputs());
    expect(spec.menus.map((menu) => menu.title)).toEqual([
      'Procyon',
      'File',
      'Edit',
      'View',
      'Tools',
      'Go',
      'Window',
      'Help',
    ]);
  });

  it('builds the App menu with Preferences and the standard AppKit roles', () => {
    const [appMenu] = buildNativeMenuSpec(inputs()).menus;
    expect(appMenu?.items).toEqual([
      { kind: 'role', role: 'about' },
      { kind: 'separator' },
      {
        kind: 'action',
        id: 'ui.openSettings',
        title: 'Preferences…',
        shortcut: { key: ',', meta: true },
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
    ]);
  });

  it('populates the File menu from registered actions by id, in the given order', () => {
    const actions = [
      action('core.closeTab', 'Close Tab', [{ key: 'w', meta: true }]),
      action('core.newTab', 'New Tab', [{ key: 't', meta: true }]),
    ];
    const fileMenu = buildNativeMenuSpec(inputs({ actions })).menus.find(
      (menu) => menu.title === 'File',
    );
    expect(fileMenu?.items).toEqual([
      {
        kind: 'action',
        id: 'core.newTab',
        title: 'New tab',
        shortcut: { key: 't', meta: true },
        enabled: true,
        checked: false,
      },
      {
        kind: 'action',
        id: 'core.closeTab',
        title: 'Close tab',
        shortcut: { key: 'w', meta: true },
        enabled: true,
        checked: false,
      },
    ]);
  });

  it('skips File menu ids that are not currently registered instead of crashing', () => {
    const fileMenu = buildNativeMenuSpec(inputs({ actions: [] })).menus.find(
      (menu) => menu.title === 'File',
    );
    expect(fileMenu?.items).toEqual([]);
  });

  it('adds New Window and Sync Workspace items first in the File menu when the host can open a window', () => {
    const fileMenu = buildNativeMenuSpec(inputs({ canOpenNewWindow: true })).menus.find(
      (menu) => menu.title === 'File',
    );
    expect(fileMenu?.items).toEqual([
      {
        kind: 'action',
        id: 'ui.newWorkspaceWindow',
        title: 'New Window',
        shortcut: { key: 'n', meta: true, shift: true },
        enabled: true,
        checked: false,
      },
      {
        kind: 'action',
        id: 'ui.syncWorkspace',
        title: 'Sync Workspace',
        enabled: true,
        checked: false,
      },
    ]);
  });

  it('omits New Window and Sync Workspace entirely on a host with no window concept', () => {
    const fileMenu = buildNativeMenuSpec(inputs({ canOpenNewWindow: false })).menus.find(
      (menu) => menu.title === 'File',
    );
    expect(
      fileMenu?.items.some((item) => 'id' in item && item.id === 'ui.newWorkspaceWindow'),
    ).toBe(false);
    expect(fileMenu?.items.some((item) => 'id' in item && item.id === 'ui.syncWorkspace')).toBe(
      false,
    );
  });

  it('restricts the Edit menu to Copy, Paste and Select All only', () => {
    const actions = [
      action('core.copy', 'Copy'),
      action('core.paste', 'Paste'),
      action('core.selectAll', 'Select All'),
      action('core.rename', 'Rename'),
    ];
    const editMenu = buildNativeMenuSpec(inputs({ actions })).menus.find(
      (menu) => menu.title === 'Edit',
    );
    expect(editMenu?.items.map((item) => (item.kind === 'action' ? item.id : item.kind))).toEqual([
      'core.copy',
      'core.paste',
      'core.selectAll',
    ]);
  });

  it('populates the View menu from the sort-order toggle actions', () => {
    const actions = [
      action('core.sortByName', 'Sort by Name'),
      action('core.sortByExtension', 'Sort by Extension'),
      action('core.sortByDate', 'Sort by Date'),
      action('core.sortBySize', 'Sort by Size'),
      action('core.sortUnsorted', 'Unsorted'),
      action('core.copy', 'Copy'),
    ];
    const viewMenu = buildNativeMenuSpec(inputs({ actions })).menus.find(
      (menu) => menu.title === 'View',
    );
    expect(viewMenu?.items.map((item) => (item.kind === 'action' ? item.id : item.kind))).toEqual([
      'core.sortByName',
      'core.sortByExtension',
      'core.sortByDate',
      'core.sortBySize',
      'core.sortUnsorted',
    ]);
  });

  it('populates the Tools menu from the copy/terminal/reveal action ids', () => {
    const actions = [
      action('core.copyName', 'Copy Filename'),
      action('core.copyPath', 'Copy Full Path'),
      action('core.copyRelativePath', 'Copy Relative Path'),
      action('core.openTerminal', 'Open Terminal Here'),
      action('core.revealInSystemFileManager', 'Reveal in Finder'),
      action('core.copy', 'Copy'),
    ];
    const toolsMenu = buildNativeMenuSpec(inputs({ actions })).menus.find(
      (menu) => menu.title === 'Tools',
    );
    expect(toolsMenu?.items.map((item) => (item.kind === 'action' ? item.id : item.kind))).toEqual([
      'ui.openSettings',
      'separator',
      'core.copyName',
      'core.copyPath',
      'core.copyRelativePath',
      'core.openTerminal',
      'core.revealInSystemFileManager',
    ]);
  });

  it('skips Tools menu ids that are not currently registered instead of crashing', () => {
    const toolsMenu = buildNativeMenuSpec(inputs({ actions: [] })).menus.find(
      (menu) => menu.title === 'Tools',
    );
    expect(toolsMenu?.items.map((item) => (item.kind === 'action' ? item.id : item.kind))).toEqual([
      'ui.openSettings',
      'separator',
    ]);
  });

  it('builds the Go menu from favourite actions, not the plain registered actions', () => {
    const favouriteActions = [
      action('core.favourites', 'Open favourites', [{ key: 'h', ctrl: true, shift: true }]),
      action('core.favourite.0', 'Open favourite: Downloads', [{ key: '1', ctrl: true }]),
    ];
    const goMenu = buildNativeMenuSpec(
      inputs({ actions: [action('core.copy', 'Copy')], favouriteActions }),
    ).menus.find((menu) => menu.title === 'Go');
    expect(goMenu?.items).toEqual([
      {
        kind: 'action',
        id: 'core.favourite.0',
        title: 'Open favourite: Downloads',
        shortcut: { key: '1', ctrl: true },
        enabled: true,
        checked: false,
      },
    ]);
  });

  it('excludes core.favourites from the Go menu (it opens the command palette, not a location)', () => {
    const favouriteActions = [
      action('core.favourites', 'Open favourites', [{ key: 'h', ctrl: true, shift: true }]),
    ];
    const goMenu = buildNativeMenuSpec(inputs({ favouriteActions })).menus.find(
      (menu) => menu.title === 'Go',
    );
    expect(goMenu?.items).toEqual([]);
  });

  it('adds Volumes/Servers/Cloud/Network groups after the favourites, each behind its own separator', () => {
    const favouriteActions = [
      action('core.favourite.0', 'Open favourite: Downloads', [{ key: '1', ctrl: true }]),
    ];
    const volumes: Volume[] = [
      { name: 'Macintosh HD', location: { providerId: 'local', uri: 'file:///' } },
    ];
    const connections: Connection[] = [connection({ id: 'connection-1', name: 'Spark' })];
    const systemLocations: SystemLocation[] = [
      {
        name: 'iCloud Drive',
        kind: 'cloud',
        location: { providerId: 'local', uri: 'file:///iCloud' },
      },
      {
        name: 'Team Files',
        kind: 'network',
        location: { providerId: 'local', uri: 'file:///Volumes/Team' },
      },
    ];
    const goMenu = buildNativeMenuSpec(
      inputs({ favouriteActions, volumes, connections, systemLocations }),
    ).menus.find((menu) => menu.title === 'Go');
    expect(goMenu?.items).toEqual([
      {
        kind: 'action',
        id: 'core.favourite.0',
        title: 'Open favourite: Downloads',
        shortcut: { key: '1', ctrl: true },
        enabled: true,
        checked: false,
      },
      { kind: 'separator' },
      {
        kind: 'action',
        id: 'ui.goMenu.volume.0',
        title: 'Macintosh HD',
        enabled: true,
        checked: false,
      },
      { kind: 'separator' },
      {
        kind: 'action',
        id: 'ui.goMenu.connection.connection-1',
        title: 'Spark (Disconnected)',
        enabled: true,
        checked: false,
      },
      { kind: 'separator' },
      {
        kind: 'action',
        id: 'ui.goMenu.systemLocation.0',
        title: 'iCloud Drive',
        enabled: true,
        checked: false,
      },
      { kind: 'separator' },
      {
        kind: 'action',
        id: 'ui.goMenu.systemLocation.1',
        title: 'Team Files',
        enabled: true,
        checked: false,
      },
    ]);
  });

  it('groups authorized OneDrive accounts with Cloud rather than Servers', () => {
    const connectionId = '11111111-1111-4111-8111-111111111111';
    const connections: Connection[] = [
      connection({
        id: connectionId,
        name: 'Work OneDrive',
        kind: 'oneDrive',
        configuration: {
          kind: 'oneDrive',
          accountHint: null,
          displayName: 'Erik Vullings',
          email: 'erik@example.test',
          driveType: 'business',
        },
        hasCredential: true,
        status: 'connected',
        rootLocation: `onedrive://${connectionId}/`,
      }),
    ];

    const goMenu = buildNativeMenuSpec(inputs({ connections })).menus.find(
      (menu) => menu.title === 'Go',
    );

    expect(goMenu?.items).toEqual([
      { kind: 'separator' },
      {
        kind: 'action',
        id: `ui.goMenu.connection.${connectionId}`,
        title: 'Work OneDrive',
        enabled: true,
        checked: false,
      },
    ]);
  });

  it('labels an unavailable volume and system location, and a disconnected browsable server', () => {
    const volumes: Volume[] = [
      { name: 'Backup Drive', location: { providerId: 'local', uri: 'file:///Volumes/Backup' } },
    ];
    const connections: Connection[] = [
      connection({ id: 'connection-1', name: 'Spark', kind: 'ssh', status: 'disconnected' }),
    ];
    const systemLocations: SystemLocation[] = [
      {
        name: 'Team Files',
        kind: 'network',
        location: { providerId: 'local', uri: 'file:///Volumes/Team' },
        readOnly: true,
      },
    ];
    const goMenu = buildNativeMenuSpec(
      inputs({
        volumes,
        connections,
        systemLocations,
        unavailableLocations: new Set(['local:file:///Volumes/Backup']),
      }),
    ).menus.find((menu) => menu.title === 'Go');
    const volumeItem = goMenu?.items.find(
      (item) => item.kind === 'action' && item.id === 'ui.goMenu.volume.0',
    );
    const connectionItem = goMenu?.items.find(
      (item) => item.kind === 'action' && item.id === 'ui.goMenu.connection.connection-1',
    );
    const systemLocationItem = goMenu?.items.find(
      (item) => item.kind === 'action' && item.id === 'ui.goMenu.systemLocation.0',
    );
    expect(volumeItem).toMatchObject({ title: 'Backup Drive (unavailable)' });
    // Disconnected but browsable (SSH): still enabled, but labelled with its status.
    expect(connectionItem).toMatchObject({ title: 'Spark (Disconnected)', enabled: true });
    expect(systemLocationItem).toMatchObject({ title: 'Team Files (read-only)' });
  });

  it('enables S3 connections in the Go-menu Servers group', () => {
    const connections: Connection[] = [
      connection({
        id: 'connection-1',
        name: 'S3 bucket',
        kind: 's3',
        status: 'disconnected',
        configuration: {
          kind: 's3',
          bucket: 'example-bucket',
          accessKeyId: 'AKIAEXAMPLE',
          region: 'us-east-1',
          endpoint: null,
          startPath: null,
        },
      }),
    ];
    const goMenu = buildNativeMenuSpec(inputs({ connections })).menus.find(
      (menu) => menu.title === 'Go',
    );
    expect(goMenu?.items).toEqual([
      { kind: 'separator' },
      {
        kind: 'action',
        id: 'ui.goMenu.connection.connection-1',
        title: 'S3 bucket (Disconnected)',
        enabled: true,
        checked: false,
      },
    ]);
  });

  it('disables a non-browsable connection kind in the Go-menu Servers group', () => {
    const connections: Connection[] = [
      connection({
        id: 'connection-1',
        name: 'Windows share',
        kind: 'smb',
        status: 'disconnected',
        configuration: {
          kind: 'smb',
          server: 'files.example.test',
          share: 'documents',
        },
      }),
    ];
    const goMenu = buildNativeMenuSpec(inputs({ connections })).menus.find(
      (menu) => menu.title === 'Go',
    );
    expect(goMenu?.items).toEqual([
      { kind: 'separator' },
      {
        kind: 'action',
        id: 'ui.goMenu.connection.connection-1',
        title: 'Windows share (Disconnected)',
        enabled: false,
        checked: false,
      },
    ]);
  });

  it('omits the Volumes/Servers/Cloud/Network groups entirely when they are empty', () => {
    const goMenu = buildNativeMenuSpec(inputs()).menus.find((menu) => menu.title === 'Go');
    expect(goMenu?.items).toEqual([]);
  });

  it('builds the Window menu with the minimize/zoom roles and one item per open tab', () => {
    const tabs = [
      tab({ tabKey: 'pane-1:tab-1', title: 'Documents', active: true }),
      tab({ tabKey: 'pane-2:tab-1', title: 'Downloads', active: false }),
    ];
    const windowMenu = buildNativeMenuSpec(inputs({ tabs })).menus.find(
      (menu) => menu.title === 'Window',
    );
    expect(windowMenu?.items).toEqual([
      { kind: 'role', role: 'minimize' },
      { kind: 'role', role: 'zoom' },
      { kind: 'separator' },
      {
        kind: 'action',
        id: 'ui.window.tab.pane-1:tab-1',
        title: 'Documents',
        enabled: true,
        checked: true,
      },
      {
        kind: 'action',
        id: 'ui.window.tab.pane-2:tab-1',
        title: 'Downloads',
        enabled: true,
        checked: false,
      },
    ]);
  });

  it('omits the tab separator entirely when there are no open tabs', () => {
    const windowMenu = buildNativeMenuSpec(inputs({ tabs: [] })).menus.find(
      (menu) => menu.title === 'Window',
    );
    expect(windowMenu?.items).toEqual([
      { kind: 'role', role: 'minimize' },
      { kind: 'role', role: 'zoom' },
    ]);
  });

  it('marks only the active tab as checked, even with tabs across multiple panes', () => {
    const tabs = [
      tab({ tabKey: 'pane-1:tab-1', active: false }),
      tab({ tabKey: 'pane-1:tab-2', active: true }),
      tab({ tabKey: 'pane-2:tab-1', active: false }),
    ];
    const windowMenu = buildNativeMenuSpec(inputs({ tabs })).menus.find(
      (menu) => menu.title === 'Window',
    );
    const checkedIds = (windowMenu?.items ?? [])
      .filter((item): item is Extract<typeof item, { kind: 'action' }> => item.kind === 'action')
      .filter((item) => item.checked)
      .map((item) => item.id);
    expect(checkedIds).toEqual(['ui.window.tab.pane-1:tab-2']);
  });

  it('adds an Open Workspace submenu listing every workspace when the host can open windows', () => {
    const workspaces = [
      workspaceSummary({ id: 'workspace-1', name: 'Default' }),
      workspaceSummary({ id: 'workspace-2', name: 'Photos' }),
    ];
    const windowMenu = buildNativeMenuSpec(
      inputs({ canOpenNewWindow: true, workspaces, currentWorkspaceId: 'workspace-2' }),
    ).menus.find((menu) => menu.title === 'Window');
    expect(windowMenu?.items).toEqual([
      { kind: 'role', role: 'minimize' },
      { kind: 'role', role: 'zoom' },
      { kind: 'separator' },
      {
        kind: 'submenu',
        title: 'Open Workspace',
        items: [
          {
            kind: 'action',
            id: 'ui.window.openWorkspace.workspace-1',
            title: 'Default',
            enabled: true,
            checked: false,
          },
          {
            kind: 'action',
            id: 'ui.window.openWorkspace.workspace-2',
            title: 'Photos',
            enabled: true,
            checked: true,
          },
        ],
      },
    ]);
  });

  it('omits the Open Workspace submenu on a host with no window concept', () => {
    const windowMenu = buildNativeMenuSpec(
      inputs({ canOpenNewWindow: false, workspaces: [workspaceSummary()] }),
    ).menus.find((menu) => menu.title === 'Window');
    expect(windowMenu?.items).toEqual([
      { kind: 'role', role: 'minimize' },
      { kind: 'role', role: 'zoom' },
    ]);
  });

  it('populates the Help menu with the shortcuts-help action when registered', () => {
    const actions = [action('core.showShortcutsHelp', 'Keyboard Shortcuts')];
    const helpMenu = buildNativeMenuSpec(inputs({ actions })).menus.find(
      (menu) => menu.title === 'Help',
    );
    expect(helpMenu?.items).toEqual([
      { kind: 'role', role: 'about' },
      {
        kind: 'action',
        id: 'ui.openDiagnostics',
        title: 'Diagnostics',
        enabled: true,
        checked: false,
      },
      { kind: 'separator' },
      {
        kind: 'action',
        id: 'core.showShortcutsHelp',
        title: 'Keyboard Shortcuts',
        shortcut: undefined,
        enabled: true,
        checked: false,
      },
    ]);
  });
});
