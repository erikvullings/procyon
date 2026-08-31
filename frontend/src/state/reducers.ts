import type {
  BackendNotification,
  ClipboardState,
  ContentMatchSummary,
  DirectoryDelta,
  DirectorySnapshot,
  EntryId,
  EntrySummary,
  Operation,
  OperationId,
  OperationProgress,
  PaneId,
  PluginDescriptor,
  RuntimeCapabilities,
  TabProjection,
  WorkspaceProjection,
  WorkspaceViewState,
} from '../models';
import type { RuntimeKind } from '../utilities/runtime';
import type {
  ConnectionState,
  DeepReadonly,
  DirectoryState,
  RuntimeState,
  WorkspaceState,
} from './model';
import type { AppPatch } from './patch';

function normalizeEntries(entries: readonly EntrySummary[]): {
  readonly entryIds: readonly EntryId[];
  readonly entriesById: Readonly<Record<EntryId, DeepReadonly<EntrySummary>>>;
} {
  const entryIds: EntryId[] = [];
  const entriesById: Record<EntryId, DeepReadonly<EntrySummary>> = {};
  for (const entry of entries) {
    if (!(entry.id in entriesById)) {
      entryIds.push(entry.id);
    }
    entriesById[entry.id] = entry;
  }
  return { entryIds, entriesById };
}

function directoryFromSnapshot(snapshot: DirectorySnapshot): DirectoryState {
  return {
    paneId: snapshot.paneId,
    sessionId: snapshot.requestId,
    requestId: snapshot.requestId,
    revision: snapshot.revision,
    writable: snapshot.writable,
    ...normalizeEntries(snapshot.entries),
  };
}

function replaceDirectorySession(
  directories: WorkspaceState['directories'],
  snapshot: DirectorySnapshot,
): WorkspaceState['directories'] {
  return {
    ...Object.fromEntries(
      Object.entries(directories).filter(([, directory]) => directory?.paneId !== snapshot.paneId),
    ),
    [snapshot.requestId]: directoryFromSnapshot(snapshot),
  };
}

/** Replaces runtime bootstrap data. */
export function runtimePatch(runtime: RuntimeState): AppPatch {
  return { runtime: () => runtime };
}

/** Convenience constructor for runtime patches at adapter boundaries. */
export function runtimeState(
  kind: RuntimeKind,
  capabilities?: DeepReadonly<RuntimeCapabilities>,
): RuntimeState {
  return capabilities === undefined ? { kind } : { kind, capabilities };
}

/** Replaces the frontend-owned in-application clipboard. */
export function clipboardPatch(clipboard: ClipboardState): AppPatch {
  return { clipboard: () => clipboard };
}

/** Replaces the workspace projection without copying or invalidating directory sessions. */
export function workspaceSnapshotPatch(workspace: WorkspaceProjection): AppPatch {
  return {
    workspace: (current) => ({ current: workspace, directories: current.directories }),
  };
}

/** Replaces one pane's directory session with a stable-ID normalized projection. */
export function directorySnapshotPatch(snapshot: DirectorySnapshot): AppPatch {
  return {
    workspace: (workspace) => ({
      ...workspace,
      directories: replaceDirectorySession(workspace.directories, snapshot),
    }),
  };
}

function upsertEntries(
  directory: DirectoryState,
  entries: readonly EntrySummary[],
  revision: number,
): DirectoryState {
  const entryIds = [...directory.entryIds];
  const entriesById = { ...directory.entriesById };
  for (const entry of entries) {
    if (!(entry.id in entriesById)) {
      entryIds.push(entry.id);
    }
    entriesById[entry.id] = entry;
  }
  return { ...directory, revision, entryIds, entriesById };
}

function removeEntries(
  directory: DirectoryState,
  entryIdsToRemove: readonly EntryId[],
  revision: number,
): DirectoryState {
  const removed = new Set(entryIdsToRemove);
  const entriesById = { ...directory.entriesById };
  for (const entryId of removed) {
    delete entriesById[entryId];
  }
  return {
    ...directory,
    revision,
    entryIds: directory.entryIds.filter((entryId) => !removed.has(entryId)),
    entriesById,
  };
}

function applyDirectoryDelta(
  workspace: WorkspaceState,
  paneId: PaneId,
  delta: DirectoryDelta,
): WorkspaceState {
  if (delta.type === 'reset') {
    return {
      ...workspace,
      directories: replaceDirectorySession(workspace.directories, delta.snapshot),
    };
  }
  const directory = Object.values(workspace.directories).find(
    (candidate) => candidate?.paneId === paneId,
  );
  if (directory === undefined) {
    return workspace;
  }
  const next =
    delta.type === 'entriesRemoved'
      ? removeEntries(directory, delta.entryIds, delta.revision)
      : upsertEntries(directory, delta.entries, delta.revision);
  return {
    ...workspace,
    directories: { ...workspace.directories, [directory.sessionId]: next },
  };
}

/** Applies an incremental directory change without relying on input ordering. */
export function directoryDeltaPatch(paneId: PaneId, delta: DirectoryDelta): AppPatch {
  return { workspace: (workspace) => applyDirectoryDelta(workspace, paneId, delta) };
}

/** Replaces transient cursor, selection, dialog and drag state independently. */
export function workspaceViewPatch(viewState: WorkspaceViewState): AppPatch {
  return { workspaceView: () => viewState };
}

/** Inserts or replaces a complete operation snapshot. */
export function operationPatch(operation: Operation): AppPatch {
  return {
    operations: (operations) => ({
      byId: { ...operations.byId, [operation.id]: operation },
    }),
  };
}

/** Replaces an operation's progress snapshot when that operation is known. */
export function operationProgressPatch(
  operationId: OperationId,
  progress: OperationProgress,
): AppPatch {
  return {
    operations: (operations) => {
      const operation = operations.byId[operationId];
      if (operation === undefined) {
        return operations;
      }
      return {
        byId: {
          ...operations.byId,
          [operationId]: { ...operation, progress },
        },
      };
    },
  };
}

/** Inserts or replaces a plugin descriptor. */
export function pluginPatch(plugin: PluginDescriptor): AppPatch {
  return {
    plugins: (plugins) => ({ byId: { ...plugins.byId, [plugin.id]: plugin } }),
  };
}

/** Appends one user-visible notification. */
export function notificationPatch(notification: BackendNotification): AppPatch {
  return {
    notifications: (notifications) => ({ items: [...notifications.items, notification] }),
  };
}

/** Replaces connection status fields as one immutable snapshot. */
export function connectionPatch(connection: ConnectionState): AppPatch {
  return { connection: () => connection };
}

/** Sets or replaces the uncommitted quick-filter draft for one tab key. */
export function setQuickFilterDraftPatch(key: string, draft: string): AppPatch {
  return { quickFilterDrafts: (s) => ({ byTabKey: { ...s.byTabKey, [key]: draft } }) };
}

/** Removes the quick-filter draft for one tab key. */
export function deleteQuickFilterDraftPatch(key: string): AppPatch {
  return {
    quickFilterDrafts: (s) => {
      const { [key]: _, ...rest } = s.byTabKey;
      return { byTabKey: rest };
    },
  };
}

/** Stores the most recently closed tab for a pane (depth-1 stack). */
export function setClosedTabStackPatch(paneId: PaneId, tab: TabProjection): AppPatch {
  return { closedTabStacks: (s) => ({ byPaneId: { ...s.byPaneId, [paneId]: tab } }) };
}

/** Removes the closed-tab record for a pane after it has been restored. */
export function deleteClosedTabStackPatch(paneId: PaneId): AppPatch {
  return {
    closedTabStacks: (s) => {
      const { [paneId]: _, ...rest } = s.byPaneId;
      return { byPaneId: rest };
    },
  };
}

/** Caches content-match summaries for an entry URI from a search SSE batch. */
export function cacheContentMatchesPatch(
  uri: string,
  matches: readonly ContentMatchSummary[],
): AppPatch {
  return { contentMatches: (s) => ({ byEntryUri: { ...s.byEntryUri, [uri]: matches } }) };
}
