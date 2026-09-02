import { describe, expect, it, vi } from 'vitest';

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
import {
  CLOSE_TAB_MENU_ID,
  dispatchNativeMenuAction,
  GO_MENU_CONNECTION_ID_PREFIX,
  GO_MENU_SYSTEM_LOCATION_ID_PREFIX,
  GO_MENU_VOLUME_ID_PREFIX,
  type NativeMenuDispatchContext,
  NEW_TAB_MENU_ID,
  NEW_WORKSPACE_WINDOW_MENU_ID,
  OPEN_DIAGNOSTICS_MENU_ID,
  OPEN_SETTINGS_MENU_ID,
  OPEN_SHORTCUTS_MENU_ID,
  SYNC_WORKSPACE_MENU_ID,
  WINDOW_OPEN_WORKSPACE_MENU_ID_PREFIX,
} from './native-menu-dispatch';

function action(id: string): ActionDescriptor {
  return {
    id,
    title: id,
    category: 'test',
    defaultShortcuts: [],
    contextRequirements: {},
    source: { kind: 'core' },
  };
}

interface ContextMocks {
  readonly findAction: ReturnType<typeof vi.fn<(id: string) => ActionDescriptor | undefined>>;
  readonly openSettingsDialog: ReturnType<typeof vi.fn<() => void>>;
  readonly openDiagnostics: ReturnType<typeof vi.fn<() => void>>;
  readonly openShortcutsHelp: ReturnType<typeof vi.fn<() => void>>;
  readonly activateTabByKey: ReturnType<typeof vi.fn<(tabKey: string) => void>>;
  readonly openNewTab: ReturnType<typeof vi.fn<(paneId: PaneId) => void>>;
  readonly closeActiveTab: ReturnType<typeof vi.fn<(paneId: PaneId) => void>>;
  readonly activePaneId: ReturnType<typeof vi.fn<() => PaneId | undefined>>;
  readonly setSort: ReturnType<
    typeof vi.fn<(paneId: PaneId, sort: readonly SortDescriptor[]) => void>
  >;
  readonly invokeAction: ReturnType<typeof vi.fn<(action: ActionDescriptor) => void>>;
  readonly openNewWorkspaceWindow: ReturnType<typeof vi.fn<() => void>>;
  readonly openWorkspaceWindowById: ReturnType<typeof vi.fn<(workspaceId: WorkspaceId) => void>>;
  readonly resyncWorkspace: ReturnType<typeof vi.fn<() => void>>;
  readonly getVolumes: ReturnType<typeof vi.fn<() => readonly Volume[]>>;
  readonly getConnections: ReturnType<typeof vi.fn<() => readonly Connection[]>>;
  readonly getSystemLocations: ReturnType<typeof vi.fn<() => readonly SystemLocation[]>>;
  readonly navigateToLocation: ReturnType<typeof vi.fn<(location: Location) => void>>;
}

function contextMocks(
  paneId?: PaneId,
  overrides: Partial<{
    volumes: readonly Volume[];
    connections: readonly Connection[];
    systemLocations: readonly SystemLocation[];
  }> = {},
): ContextMocks & { readonly context: NativeMenuDispatchContext } {
  const findAction = vi.fn<(id: string) => ActionDescriptor | undefined>(() => undefined);
  const openSettingsDialog = vi.fn<() => void>();
  const openDiagnostics = vi.fn<() => void>();
  const openShortcutsHelp = vi.fn<() => void>();
  const activateTabByKey = vi.fn<(tabKey: string) => void>();
  const openNewTab = vi.fn<(paneId: PaneId) => void>();
  const closeActiveTab = vi.fn<(paneId: PaneId) => void>();
  const activePaneId = vi.fn<() => PaneId | undefined>(() => paneId);
  const setSort = vi.fn<(paneId: PaneId, sort: readonly SortDescriptor[]) => void>();
  const invokeAction = vi.fn<(action: ActionDescriptor) => void>();
  const openNewWorkspaceWindow = vi.fn<() => void>();
  const openWorkspaceWindowById = vi.fn<(workspaceId: WorkspaceId) => void>();
  const resyncWorkspace = vi.fn<() => void>();
  const getVolumes = vi.fn<() => readonly Volume[]>(() => overrides.volumes ?? []);
  const getConnections = vi.fn<() => readonly Connection[]>(() => overrides.connections ?? []);
  const getSystemLocations = vi.fn<() => readonly SystemLocation[]>(
    () => overrides.systemLocations ?? [],
  );
  const navigateToLocation = vi.fn<(location: Location) => void>();
  return {
    findAction,
    openSettingsDialog,
    openDiagnostics,
    openShortcutsHelp,
    activateTabByKey,
    openNewTab,
    closeActiveTab,
    activePaneId,
    setSort,
    invokeAction,
    openNewWorkspaceWindow,
    openWorkspaceWindowById,
    resyncWorkspace,
    getVolumes,
    getConnections,
    getSystemLocations,
    navigateToLocation,
    context: {
      findAction,
      openSettingsDialog,
      openDiagnostics,
      openShortcutsHelp,
      activateTabByKey,
      openNewTab,
      closeActiveTab,
      activePaneId,
      setSort,
      invokeAction,
      getVolumes,
      getConnections,
      getSystemLocations,
      navigateToLocation,
      openNewWorkspaceWindow,
      openWorkspaceWindowById,
      resyncWorkspace,
    },
  };
}

describe('dispatchNativeMenuAction', () => {
  it('opens keyboard shortcut help through the frontend path', () => {
    const mocks = contextMocks();
    dispatchNativeMenuAction(mocks.context, OPEN_SHORTCUTS_MENU_ID);
    expect(mocks.openShortcutsHelp).toHaveBeenCalledOnce();
    expect(mocks.invokeAction).not.toHaveBeenCalled();
  });

  it('opens diagnostics for the frontend-local Help menu id', () => {
    const mocks = contextMocks();
    dispatchNativeMenuAction(mocks.context, OPEN_DIAGNOSTICS_MENU_ID);
    expect(mocks.openDiagnostics).toHaveBeenCalledOnce();
    expect(mocks.invokeAction).not.toHaveBeenCalled();
  });
  it('opens a new tab in the active pane through the local tab controller', () => {
    const mocks = contextMocks('pane-1' as PaneId);
    dispatchNativeMenuAction(mocks.context, NEW_TAB_MENU_ID);
    expect(mocks.openNewTab).toHaveBeenCalledExactlyOnceWith('pane-1');
    expect(mocks.invokeAction).not.toHaveBeenCalled();
  });

  it('closes the active tab through the local tab controller', () => {
    const mocks = contextMocks('pane-1' as PaneId);
    dispatchNativeMenuAction(mocks.context, CLOSE_TAB_MENU_ID);
    expect(mocks.closeActiveTab).toHaveBeenCalledExactlyOnceWith('pane-1');
    expect(mocks.invokeAction).not.toHaveBeenCalled();
  });
  it('opens Settings for the frontend-local ui.openSettings id without touching the registry', () => {
    const mocks = contextMocks();
    dispatchNativeMenuAction(mocks.context, OPEN_SETTINGS_MENU_ID);
    expect(mocks.openSettingsDialog).toHaveBeenCalledOnce();
    expect(mocks.activateTabByKey).not.toHaveBeenCalled();
    expect(mocks.invokeAction).not.toHaveBeenCalled();
  });

  it('activates the tab encoded after the ui.window.tab. prefix', () => {
    const mocks = contextMocks();
    dispatchNativeMenuAction(mocks.context, 'ui.window.tab.pane-1:tab-2');
    expect(mocks.activateTabByKey).toHaveBeenCalledExactlyOnceWith('pane-1:tab-2');
    expect(mocks.openSettingsDialog).not.toHaveBeenCalled();
    expect(mocks.invokeAction).not.toHaveBeenCalled();
  });

  it('opens a new workspace window for the frontend-local ui.newWorkspaceWindow id', () => {
    const mocks = contextMocks();
    dispatchNativeMenuAction(mocks.context, NEW_WORKSPACE_WINDOW_MENU_ID);
    expect(mocks.openNewWorkspaceWindow).toHaveBeenCalledOnce();
    expect(mocks.invokeAction).not.toHaveBeenCalled();
  });

  it('resyncs the workspace for the frontend-local ui.syncWorkspace id', () => {
    const mocks = contextMocks();
    dispatchNativeMenuAction(mocks.context, SYNC_WORKSPACE_MENU_ID);
    expect(mocks.resyncWorkspace).toHaveBeenCalledOnce();
    expect(mocks.invokeAction).not.toHaveBeenCalled();
  });

  it('opens the given workspace window for the ui.window.openWorkspace. prefix', () => {
    const mocks = contextMocks();
    dispatchNativeMenuAction(mocks.context, `${WINDOW_OPEN_WORKSPACE_MENU_ID_PREFIX}workspace-2`);
    expect(mocks.openWorkspaceWindowById).toHaveBeenCalledExactlyOnceWith('workspace-2');
    expect(mocks.invokeAction).not.toHaveBeenCalled();
  });

  it('looks up any other id in the action registry and invokes it via invokePaletteAction', () => {
    const copy = action('core.copy');
    const mocks = contextMocks();
    mocks.findAction.mockImplementation((id) => (id === 'core.copy' ? copy : undefined));
    dispatchNativeMenuAction(mocks.context, 'core.copy');
    expect(mocks.invokeAction).toHaveBeenCalledExactlyOnceWith(copy);
  });

  it('silently no-ops for a stale id no longer present in the registry', () => {
    const mocks = contextMocks();
    expect(() => dispatchNativeMenuAction(mocks.context, 'core.longRemovedAction')).not.toThrow();
    expect(mocks.invokeAction).not.toHaveBeenCalled();
    expect(mocks.openSettingsDialog).not.toHaveBeenCalled();
    expect(mocks.activateTabByKey).not.toHaveBeenCalled();
    expect(mocks.openNewWorkspaceWindow).not.toHaveBeenCalled();
  });

  it('applies a sort-menu id as a local setSort call to the active pane, not a registry dispatch', () => {
    const mocks = contextMocks('pane-1' as PaneId);
    dispatchNativeMenuAction(mocks.context, 'core.sortByName');
    expect(mocks.setSort).toHaveBeenCalledExactlyOnceWith('pane-1', [
      { columnId: 'core.name', direction: 'ascending' },
    ]);
    expect(mocks.invokeAction).not.toHaveBeenCalled();
    expect(mocks.findAction).not.toHaveBeenCalled();
  });

  it('applies core.sortUnsorted as an empty sort', () => {
    const mocks = contextMocks('pane-1' as PaneId);
    dispatchNativeMenuAction(mocks.context, 'core.sortUnsorted');
    expect(mocks.setSort).toHaveBeenCalledExactlyOnceWith('pane-1', []);
  });

  it('no-ops a sort-menu click when there is no active pane', () => {
    const mocks = contextMocks(undefined);
    dispatchNativeMenuAction(mocks.context, 'core.sortByName');
    expect(mocks.setSort).not.toHaveBeenCalled();
  });

  it('navigates to the volume at the id-encoded index for a Go-menu Volumes item', () => {
    const volume: Volume = {
      name: 'Macintosh HD',
      location: { providerId: 'local', uri: 'file:///' },
    };
    const mocks = contextMocks(undefined, { volumes: [volume] });
    dispatchNativeMenuAction(mocks.context, `${GO_MENU_VOLUME_ID_PREFIX}0`);
    expect(mocks.navigateToLocation).toHaveBeenCalledExactlyOnceWith(volume.location);
  });

  it('silently no-ops a Volumes item id whose index is out of range', () => {
    const mocks = contextMocks(undefined, { volumes: [] });
    expect(() =>
      dispatchNativeMenuAction(mocks.context, `${GO_MENU_VOLUME_ID_PREFIX}0`),
    ).not.toThrow();
    expect(mocks.navigateToLocation).not.toHaveBeenCalled();
  });

  it('navigates to a browsable connection for a Go-menu Servers item', () => {
    const connection: Connection = {
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
      status: 'connected',
      createdAt: '2026-01-01T00:00:00.000Z',
      updatedAt: '2026-01-01T00:00:00.000Z',
    };
    const mocks = contextMocks(undefined, { connections: [connection] });
    dispatchNativeMenuAction(mocks.context, `${GO_MENU_CONNECTION_ID_PREFIX}connection-1`);
    expect(mocks.navigateToLocation).toHaveBeenCalledExactlyOnceWith({
      providerId: 'sftp',
      uri: 'sftp://connection-1/',
    });
  });

  it('navigates to an S3 connection for a Go-menu Servers item', () => {
    const connection: Connection = {
      id: 'connection-1',
      name: 'S3 bucket',
      kind: 's3',
      configuration: {
        kind: 's3',
        bucket: 'example-bucket',
        accessKeyId: 'AKIAEXAMPLE',
        region: 'us-east-1',
        endpoint: null,
        startPath: null,
      },
      hasCredential: true,
      status: 'disconnected',
      createdAt: '2026-01-01T00:00:00.000Z',
      updatedAt: '2026-01-01T00:00:00.000Z',
    };
    const mocks = contextMocks(undefined, { connections: [connection] });
    dispatchNativeMenuAction(mocks.context, `${GO_MENU_CONNECTION_ID_PREFIX}connection-1`);
    expect(mocks.navigateToLocation).toHaveBeenCalledExactlyOnceWith({
      providerId: 's3',
      uri: 's3://connection-1/',
    });
  });

  it('does not navigate for an unbrowsable connection kind (task 0144)', () => {
    const connection: Connection = {
      id: 'connection-1',
      name: 'Windows share',
      kind: 'smb',
      configuration: {
        kind: 'smb',
        server: 'files.example.test',
        share: 'documents',
      },
      hasCredential: true,
      status: 'disconnected',
      createdAt: '2026-01-01T00:00:00.000Z',
      updatedAt: '2026-01-01T00:00:00.000Z',
    };
    const mocks = contextMocks(undefined, { connections: [connection] });
    dispatchNativeMenuAction(mocks.context, `${GO_MENU_CONNECTION_ID_PREFIX}connection-1`);
    expect(mocks.navigateToLocation).not.toHaveBeenCalled();
  });

  it('navigates to the system location at the id-encoded index for a Go-menu Cloud/Network item', () => {
    const systemLocation: SystemLocation = {
      name: 'Team Files',
      kind: 'network',
      location: { providerId: 'local', uri: 'file:///Volumes/Team%20Files' },
    };
    const mocks = contextMocks(undefined, { systemLocations: [systemLocation] });
    dispatchNativeMenuAction(mocks.context, `${GO_MENU_SYSTEM_LOCATION_ID_PREFIX}0`);
    expect(mocks.navigateToLocation).toHaveBeenCalledExactlyOnceWith(systemLocation.location);
  });
});
