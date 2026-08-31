import { describe, expect, it, vi } from 'vitest';

import type { FileManagerClient } from '../../api/client/file-manager-client';
import type { Location, PaneId, TabId, WorkspaceProjection } from '../../models';
import type { NavigationController } from '../navigation/navigation';
import { createTabController, type TabControllerContext } from './tab-controller';

function projection(
  overrides: Partial<{
    readonly tabOrderByPane: Readonly<Record<string, readonly TabId[]>>;
    readonly activeTabIdByPane: Readonly<Record<string, TabId>>;
  }> = {},
): WorkspaceProjection {
  const tabOrder = overrides.tabOrderByPane?.['pane-1'] ?? ['tab-1', 'tab-2', 'tab-3'];
  const activeTabId = overrides.activeTabIdByPane?.['pane-1'] ?? 'tab-1';
  const tabsById: WorkspaceProjection['panesById']['pane-1']['tabsById'] = {};
  for (const tabId of tabOrder) {
    tabsById[tabId] = {
      id: tabId,
      title: tabId,
      location: { providerId: 'local', uri: `file:///${tabId}` },
      canNavigateBack: false,
      canNavigateForward: false,
      view: { sort: [], columns: [], showHidden: false, foldersFirst: true, quickFilter: null },
    };
  }
  return {
    id: 'workspace-1',
    name: 'Workspace',
    revision: 1,
    layout: { type: 'pane', paneId: 'pane-1' },
    paneOrder: ['pane-1'],
    panesById: {
      'pane-1': { id: 'pane-1', tabOrder: [...tabOrder], tabsById, activeTabId },
    },
    activePaneId: 'pane-1',
    operationCentre: { visible: false, height: 180 },
    ephemeral: false,
  };
}

function makeContext(workspace: WorkspaceProjection): TabControllerContext {
  let current = workspace;
  return {
    getWorkspace: () => current,
    setWorkspace: (ws) => {
      current = ws;
    },
    getAppState: () => undefined,
    setAppState: vi.fn(),
    getNavigation: () =>
      ({
        load: vi.fn().mockResolvedValue(undefined),
        abort: vi.fn(),
      }) as unknown as NavigationController,
    redraw: vi.fn(),
    applyCurrentShowHiddenSetting: vi.fn().mockResolvedValue(undefined),
    clearTabState: vi.fn(),
    getCloseTabConfirmation: () => undefined,
    setCloseTabConfirmation: vi.fn(),
    hasCachedSnapshot: () => false,
  };
}

describe('createTabController', () => {
  it('activateTab refreshes the listing instead of no-opping when the tab is already active', () => {
    // Regression test: re-clicking a tab that's already active used to be a pure no-op, so an
    // external filesystem change (e.g. a browser download landing in a folder the user is already
    // looking at) never showed up without a manual hard reload.
    const workspace = projection({ activeTabIdByPane: { 'pane-1': 'tab-1' } });
    const context = makeContext(workspace);
    const loadSpy = vi.fn().mockResolvedValue(undefined);
    context.getNavigation = () =>
      ({ load: loadSpy, abort: vi.fn() }) as unknown as NavigationController;
    const client = {
      dispatchWorkspaceCommand: vi.fn(),
      getWorkspace: vi.fn(),
    } as unknown as FileManagerClient;
    const controller = createTabController(client, context);

    controller.activateTab('pane-1' as PaneId, 'tab-1' as TabId);

    expect(loadSpy).toHaveBeenCalledWith('pane-1', { background: true });
    expect(client.dispatchWorkspaceCommand).not.toHaveBeenCalled();
  });

  it('openTabAt adds a tab at an arbitrary location rather than duplicating the active tab', async () => {
    const workspace = projection();
    const context = makeContext(workspace);
    let dispatchedLocation: Location | undefined;
    const client = {
      dispatchWorkspaceCommand: vi.fn((command) => {
        if (command.type === 'addTab') dispatchedLocation = command.location;
        const next = projection({
          tabOrderByPane: { 'pane-1': ['tab-1', 'tab-2', 'tab-3', 'tab-4'] },
        });
        return Promise.resolve({ ...next, activePaneId: 'pane-1' });
      }),
      getWorkspace: vi.fn(),
    } as unknown as FileManagerClient;
    const controller = createTabController(client, context);
    const target: Location = { providerId: 'local', uri: 'file:///target-dir' };

    controller.openTabAt('pane-1' as PaneId, target);
    await Promise.resolve();
    await Promise.resolve();

    expect(dispatchedLocation).toEqual(target);
  });

  it('openTabAt establishes a durable origin before navigating to a transient result', async () => {
    const context = makeContext(projection());
    const navigate = vi.fn().mockResolvedValue(undefined);
    context.getNavigation = () =>
      ({ navigate, load: vi.fn(), abort: vi.fn() }) as unknown as NavigationController;
    let dispatchedLocation: Location | undefined;
    const client = {
      dispatchWorkspaceCommand: vi.fn((command) => {
        if (command.type === 'addTab') dispatchedLocation = command.location;
        return Promise.resolve(
          projection({
            tabOrderByPane: { 'pane-1': ['tab-1', 'tab-2', 'tab-3', 'tab-4'] },
            activeTabIdByPane: { 'pane-1': 'tab-4' },
          }),
        );
      }),
      getWorkspace: vi.fn(),
    } as unknown as FileManagerClient;
    const result: Location = {
      providerId: 'search',
      uri: 'search://local/new-search',
    };
    const origin: Location = { providerId: 'local', uri: 'file:///Documents' };

    createTabController(client, context).openTabAt('pane-1' as PaneId, result, origin);
    await vi.waitFor(() => expect(navigate).toHaveBeenCalled());

    expect(dispatchedLocation).toEqual(origin);
    expect(navigate).toHaveBeenCalledWith('pane-1', result);
  });

  it('closeAllTabs closes every tab except the active one', async () => {
    const workspace = projection();
    const context = makeContext(workspace);
    const closedTabIds: TabId[] = [];
    const client = {
      dispatchWorkspaceCommand: vi.fn((command) => {
        if (command.type === 'closeTab') {
          closedTabIds.push(command.tabId);
          const pane = context.getWorkspace()?.panesById['pane-1'];
          const remaining = (pane === undefined ? [] : pane.tabOrder).filter(
            (id) => id !== command.tabId,
          );
          return Promise.resolve(
            projection({
              tabOrderByPane: { 'pane-1': remaining },
              activeTabIdByPane: { 'pane-1': 'tab-1' },
            }),
          );
        }
        return Promise.reject(new Error('unexpected command'));
      }),
      getWorkspace: vi.fn(),
    } as unknown as FileManagerClient;
    const controller = createTabController(client, context);

    controller.closeAllTabs('pane-1' as PaneId);
    // Let the sequential close chain (a `.reduce` over promises) settle.
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();

    expect(closedTabIds.sort()).toEqual(['tab-2', 'tab-3']);
  });
});
