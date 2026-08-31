import type { FileManagerClient } from '../../api/client/file-manager-client';
import type { Location, PaneId, TabId, WorkspaceProjection } from '../../models';
import {
  type AppState,
  applyAppPatches,
  deleteClosedTabStackPatch,
  setClosedTabStackPatch,
} from '../../state';
import type { NavigationController } from '../navigation/navigation';
import { dispatchWorkspaceCommand } from '../workspace/dispatch-workspace-command';
import { cycledTabIndex, tabIdForJump } from './tab-navigation';

export interface TabControllerContext {
  getWorkspace(): WorkspaceProjection | undefined;
  setWorkspace(ws: WorkspaceProjection): void;
  getAppState(): AppState | undefined;
  setAppState(state: AppState): void;
  getNavigation(): NavigationController;
  redraw(): void;
  applyCurrentShowHiddenSetting(
    client: FileManagerClient,
    workspaceId: string,
    paneId: PaneId,
    tabId: TabId,
    revision: number,
  ): Promise<void>;
  clearTabState(paneId: PaneId, tabId: TabId): void;
  getCloseTabConfirmation(): { readonly paneId: PaneId; readonly tabId: TabId } | undefined;
  setCloseTabConfirmation(conf?: { readonly paneId: PaneId; readonly tabId: TabId }): void;
  hasCachedSnapshot(paneId: PaneId, tabId: TabId): boolean;
}

export interface TabController {
  openTab(paneId: PaneId): void;
  /** Opens a new tab at an arbitrary location rather than duplicating the active tab. */
  openTabAt(paneId: PaneId, location: Location, historyOrigin?: Location): void;
  activateTab(paneId: PaneId, tabId: TabId): void;
  performCloseTab(paneId: PaneId, tabId: TabId): void;
  requestCloseTab(paneId: PaneId, tabId: TabId): void;
  /** Closes every tab in `paneId` except the active one (Ctrl+Shift+W). */
  closeAllTabs(paneId: PaneId): void;
  reopenClosedTab(paneId: PaneId): void;
  cycleTab(paneId: PaneId, direction: 1 | -1): void;
  jumpToTab(paneId: PaneId, oneBasedIndex: number): void;
}

export function createTabController(
  client: FileManagerClient,
  context: TabControllerContext,
): TabController {
  function replaceWorkspace(next: WorkspaceProjection): void {
    context.setWorkspace(next);
  }

  return {
    openTab(paneId: PaneId): void {
      const workspace = context.getWorkspace();
      if (workspace === undefined) return;
      const pane = workspace.panesById[paneId];
      const activeTab = pane?.tabsById[pane.activeTabId];
      if (activeTab === undefined) return;
      void dispatchWorkspaceCommand(
        client,
        {
          type: 'addTab',
          workspaceId: workspace.id,
          paneId,
          location: activeTab.location,
          expectedRevision: workspace.revision,
        },
        (next) => {
          replaceWorkspace(next);
          const newTabId = next.panesById[paneId]?.activeTabId;
          if (newTabId === undefined) {
            void context.getNavigation().load(paneId);
            return;
          }
          void context
            .applyCurrentShowHiddenSetting(client, next.id, paneId, newTabId, next.revision)
            .then(() => context.getNavigation().load(paneId));
        },
      ).catch(() => undefined);
    },

    openTabAt(paneId: PaneId, location: Location, historyOrigin?: Location): void {
      const workspace = context.getWorkspace();
      if (workspace === undefined) return;
      void dispatchWorkspaceCommand(
        client,
        {
          type: 'addTab',
          workspaceId: workspace.id,
          paneId,
          location: historyOrigin ?? location,
          expectedRevision: workspace.revision,
        },
        (next) => {
          replaceWorkspace(next);
          if (historyOrigin !== undefined) {
            void context.getNavigation().navigate(paneId, location);
            return;
          }
          const newTabId = next.panesById[paneId]?.activeTabId;
          if (newTabId === undefined) {
            void context.getNavigation().load(paneId);
            return;
          }
          void context
            .applyCurrentShowHiddenSetting(client, next.id, paneId, newTabId, next.revision)
            .then(() => context.getNavigation().load(paneId));
        },
      ).catch(() => undefined);
    },

    activateTab(paneId: PaneId, tabId: TabId): void {
      const workspace = context.getWorkspace();
      if (workspace === undefined) return;
      const pane = workspace.panesById[paneId];
      if (pane === undefined) return;
      if (pane.activeTabId === tabId) {
        // Re-clicking the tab that's already active isn't a tab switch, so there's no workspace
        // state to change - but the user is deliberately revisiting this listing (e.g. after an
        // external change like a browser download landed while it sat idle), so still refresh it
        // rather than silently no-op-ing and leaving stale entries on screen.
        void context.getNavigation().load(paneId, { background: true });
        return;
      }
      const previousTabId = pane.activeTabId;
      context.setWorkspace({
        ...workspace,
        activePaneId: paneId,
        panesById: {
          ...workspace.panesById,
          [paneId]: { ...pane, activeTabId: tabId },
        },
      });
      // Task 0069's acceptance criteria: "switching tabs is instant: the previous snapshot is
      // reused if still valid, otherwise refetched."
      const hasCachedSnapshot = context.hasCachedSnapshot(paneId, tabId);
      void dispatchWorkspaceCommand(
        client,
        {
          type: 'activateTab',
          workspaceId: workspace.id,
          paneId,
          tabId,
          expectedRevision: workspace.revision,
        },
        (next) => {
          replaceWorkspace(next);
          context.getNavigation().abort(paneId, previousTabId);
          void context
            .getNavigation()
            .load(paneId, hasCachedSnapshot ? { background: true } : undefined);
        },
      ).catch(() => {
        const current = context.getWorkspace();
        if (current?.revision === workspace.revision) context.setWorkspace(workspace);
      });
    },

    performCloseTab(paneId: PaneId, tabId: TabId): void {
      const workspace = context.getWorkspace();
      if (workspace === undefined) return;
      const closedTab = workspace.panesById[paneId]?.tabsById[tabId];
      let appState = context.getAppState();
      if (closedTab !== undefined && appState !== undefined) {
        appState = applyAppPatches(appState, setClosedTabStackPatch(paneId, closedTab));
        context.setAppState(appState);
      }
      void dispatchWorkspaceCommand(
        client,
        {
          type: 'closeTab',
          workspaceId: workspace.id,
          paneId,
          tabId,
          expectedRevision: workspace.revision,
        },
        (next) => {
          context.clearTabState(paneId, tabId);
          replaceWorkspace(next);
          void context.getNavigation().load(paneId);
        },
      ).catch(() => undefined);
    },

    requestCloseTab(paneId: PaneId, tabId: TabId): void {
      const workspace = context.getWorkspace();
      const pane = workspace?.panesById[paneId];
      if (pane === undefined) return;
      if (pane.tabOrder.length <= 1) {
        context.setCloseTabConfirmation({ paneId, tabId });
        context.redraw();
        return;
      }
      this.performCloseTab(paneId, tabId);
    },

    closeAllTabs(paneId: PaneId): void {
      const workspace = context.getWorkspace();
      const pane = workspace?.panesById[paneId];
      if (workspace === undefined || pane === undefined) return;
      const idsToClose = pane.tabOrder.filter((tabId) => tabId !== pane.activeTabId);
      // Closed sequentially (not fired concurrently) so each command's `expectedRevision`
      // matches the workspace revision left by the previous close.
      void idsToClose
        .reduce<Promise<void>>(
          (chain, tabId) =>
            chain.then(async () => {
              const current = context.getWorkspace();
              if (current === undefined) return;
              const closedTab = current.panesById[paneId]?.tabsById[tabId];
              let appState = context.getAppState();
              if (closedTab !== undefined && appState !== undefined) {
                appState = applyAppPatches(appState, setClosedTabStackPatch(paneId, closedTab));
                context.setAppState(appState);
              }
              try {
                await dispatchWorkspaceCommand(
                  client,
                  {
                    type: 'closeTab',
                    workspaceId: current.id,
                    paneId,
                    tabId,
                    expectedRevision: current.revision,
                  },
                  replaceWorkspace,
                );
                context.clearTabState(paneId, tabId);
              } catch {
                // A stale revision or already-closed tab shouldn't abort the remaining closes.
              }
            }),
          Promise.resolve(),
        )
        .then(() => context.getNavigation().load(paneId));
    },

    reopenClosedTab(paneId: PaneId): void {
      const workspace = context.getWorkspace();
      let appState = context.getAppState();
      const closed = appState?.closedTabStacks.byPaneId[paneId];
      if (workspace === undefined || appState === undefined || closed === undefined) return;
      appState = applyAppPatches(appState, deleteClosedTabStackPatch(paneId));
      context.setAppState(appState);
      void dispatchWorkspaceCommand(
        client,
        {
          type: 'addTab',
          workspaceId: workspace.id,
          paneId,
          location: closed.location,
          expectedRevision: workspace.revision,
        },
        (next) => {
          replaceWorkspace(next);
          const newTabId = next.panesById[paneId]?.activeTabId;
          if (newTabId === undefined) {
            void context.getNavigation().load(paneId);
            return;
          }
          void context
            .applyCurrentShowHiddenSetting(client, next.id, paneId, newTabId, next.revision)
            .then(() => context.getNavigation().load(paneId));
        },
      ).catch(() => undefined);
    },

    cycleTab(paneId: PaneId, direction: 1 | -1): void {
      const workspace = context.getWorkspace();
      const pane = workspace?.panesById[paneId];
      if (pane === undefined) return;
      const currentIndex = pane.tabOrder.indexOf(pane.activeTabId);
      const nextTabId =
        pane.tabOrder[cycledTabIndex(currentIndex, pane.tabOrder.length, direction)];
      if (nextTabId !== undefined) this.activateTab(paneId, nextTabId);
    },

    jumpToTab(paneId: PaneId, oneBasedIndex: number): void {
      const workspace = context.getWorkspace();
      const pane = workspace?.panesById[paneId];
      if (pane === undefined) return;
      const tabId = tabIdForJump(pane.tabOrder, oneBasedIndex);
      if (tabId !== undefined) this.activateTab(paneId, tabId);
    },
  };
}
