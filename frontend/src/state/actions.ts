import type {
  BackendNotification,
  ClipboardState,
  ContentMatchSummary,
  DirectoryDelta,
  DirectorySnapshot,
  Operation,
  OperationId,
  OperationProgress,
  PaneId,
  PluginDescriptor,
  TabProjection,
  WorkspaceProjection,
  WorkspaceViewState,
} from '../models';
import type { ConnectionState, RuntimeState } from './model';
import type { AppUpdate } from './patch';
import {
  cacheContentMatchesPatch,
  clipboardPatch,
  connectionPatch,
  deleteClosedTabStackPatch,
  deleteQuickFilterDraftPatch,
  directoryDeltaPatch,
  directorySnapshotPatch,
  notificationPatch,
  operationPatch,
  operationProgressPatch,
  pluginPatch,
  runtimePatch,
  setClosedTabStackPatch,
  setQuickFilterDraftPatch,
  workspaceSnapshotPatch,
  workspaceViewPatch,
} from './reducers';

/** Typed mutations available to components and backend-event producers. */
export interface AppActions {
  setRuntime(runtime: RuntimeState): void;
  replaceClipboard(clipboard: ClipboardState): void;
  replaceWorkspace(workspace: WorkspaceProjection): void;
  replaceWorkspaceView(viewState: WorkspaceViewState): void;
  replaceDirectory(snapshot: DirectorySnapshot): void;
  applyDirectoryDelta(paneId: PaneId, delta: DirectoryDelta): void;
  upsertOperation(operation: Operation): void;
  updateOperationProgress(operationId: OperationId, progress: OperationProgress): void;
  upsertPlugin(plugin: PluginDescriptor): void;
  notify(notification: BackendNotification): void;
  setConnection(connection: ConnectionState): void;
  setQuickFilterDraft(key: string, draft: string): void;
  deleteQuickFilterDraft(key: string): void;
  setClosedTabStack(paneId: PaneId, tab: TabProjection): void;
  deleteClosedTabStack(paneId: PaneId): void;
  cacheContentMatches(uri: string, matches: readonly ContentMatchSummary[]): void;
}

/** Binds pure slice reducers to the store's single batched update boundary. */
export function createAppActions(update: AppUpdate): AppActions {
  return {
    setRuntime: (runtime) => update(runtimePatch(runtime)),
    replaceClipboard: (clipboard) => update(clipboardPatch(clipboard)),
    replaceWorkspace: (workspace) => update(workspaceSnapshotPatch(workspace)),
    replaceWorkspaceView: (viewState) => update(workspaceViewPatch(viewState)),
    replaceDirectory: (snapshot) => update(directorySnapshotPatch(snapshot)),
    applyDirectoryDelta: (paneId, delta) => update(directoryDeltaPatch(paneId, delta)),
    upsertOperation: (operation) => update(operationPatch(operation)),
    updateOperationProgress: (operationId, progress) =>
      update(operationProgressPatch(operationId, progress)),
    upsertPlugin: (plugin) => update(pluginPatch(plugin)),
    notify: (notification) => update(notificationPatch(notification)),
    setConnection: (connection) => update(connectionPatch(connection)),
    setQuickFilterDraft: (key, draft) => update(setQuickFilterDraftPatch(key, draft)),
    deleteQuickFilterDraft: (key) => update(deleteQuickFilterDraftPatch(key)),
    setClosedTabStack: (paneId, tab) => update(setClosedTabStackPatch(paneId, tab)),
    deleteClosedTabStack: (paneId) => update(deleteClosedTabStackPatch(paneId)),
    cacheContentMatches: (uri, matches) => update(cacheContentMatchesPatch(uri, matches)),
  };
}
