import { describe, expect, it } from 'vitest';

import type {
  DirectorySnapshot,
  EntrySummary,
  Operation,
  PluginDescriptor,
  TabProjection,
  WorkspaceProjection,
} from '../models';
import { createInitialAppState } from './model';
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
import { applyAppPatches } from './store';

const location = { providerId: 'file', uri: 'file:///tmp' } as const;

function entry(id: string, name = id): EntrySummary {
  return {
    id,
    location,
    name,
    kind: 'file',
    hidden: false,
    readOnly: false,
    metadataRevision: 1,
  };
}

function workspace(name: string): WorkspaceProjection {
  return {
    id: 'workspace-1',
    name,
    revision: 1,
    paneOrder: [],
    panesById: {},
    activePaneId: 'pane-1',
    layout: { type: 'pane', paneId: 'pane-1' },
    operationCentre: { visible: false, height: 180 },
    ephemeral: false,
  };
}

function snapshot(entries: EntrySummary[], revision = 1): DirectorySnapshot {
  return {
    paneId: 'pane-1',
    requestId: `request-${revision}`,
    revision,
    location,
    writable: true,
    entries,
    hasMore: false,
    loadingState: { type: 'loaded' },
  };
}

function operation(): Operation {
  return {
    id: 'operation-1',
    kind: 'copy',
    state: 'running',
    sources: [],
    progress: { completedItems: 0, completedBytes: 0 },
    conflictPolicy: 'ask',
    createdAt: '2026-07-30T00:00:00Z',
  };
}

describe('state slice reducers', () => {
  it('keeps in-application clipboard references in their own state slice', () => {
    const state = applyAppPatches(
      createInitialAppState('mock'),
      clipboardPatch({ mode: 'move', locations: [{ providerId: 'file', uri: 'file:///tmp/a' }] }),
    );

    expect(state.clipboard).toEqual({
      mode: 'move',
      locations: [{ providerId: 'file', uri: 'file:///tmp/a' }],
    });
  });

  it('replaces major workspace snapshots wholesale', () => {
    const initial = applyAppPatches(
      createInitialAppState('mock'),
      workspaceSnapshotPatch(workspace('Before')),
    );
    const beforeWorkspace = initial.workspace;
    const next = applyAppPatches(initial, workspaceSnapshotPatch(workspace('After')));

    expect(next.workspace.current?.name).toBe('After');
    expect(next.workspace.directories).toBe(initial.workspace.directories);
    expect(beforeWorkspace.current?.name).toBe('Before');
  });

  it('leaves previously stored directory entries untouched by a workspace mutation', () => {
    const withDirectory = applyAppPatches(
      createInitialAppState('mock'),
      directorySnapshotPatch(snapshot([entry('a'), entry('b')])),
    );
    const entries = withDirectory.workspace.directories['request-1']?.entriesById;
    const next = applyAppPatches(
      withDirectory,
      workspaceSnapshotPatch(workspace('After mutation')),
    );

    expect(next.workspace.directories['request-1']?.entriesById).toBe(entries);
    expect(next.workspace.directories['request-1']?.entryIds).toEqual(['a', 'b']);
  });

  it('keeps cursor and selection in a frontend-only slice', () => {
    const projection = workspace('Workspace');
    const state = applyAppPatches(
      createInitialAppState('mock'),
      workspaceSnapshotPatch(projection),
      workspaceViewPatch({
        focusedPaneId: 'pane-1',
        paneViews: {
          'pane-1': { selectedEntryIds: ['entry-1'], cursorEntryId: 'entry-1' },
        },
      }),
    );

    expect(state.workspaceView?.paneViews['pane-1']?.selectedEntryIds).toEqual(['entry-1']);
    expect(state.workspace.current).toEqual(projection);
    expect(state.workspace.current).not.toHaveProperty('paneViews');
  });

  it('keys directory entries by stable EntryId and immutably applies interleaved deltas', () => {
    const initial = applyAppPatches(
      createInitialAppState('mock'),
      directorySnapshotPatch(snapshot([entry('b'), entry('a')])),
    );
    const beforeDirectory = initial.workspace.directories['request-1'];
    const updated = applyAppPatches(
      initial,
      directoryDeltaPatch('pane-1', {
        type: 'entriesUpdated',
        revision: 2,
        entries: [entry('a', 'A2'), entry('b', 'B2')],
      }),
      directoryDeltaPatch('pane-1', {
        type: 'entriesRemoved',
        revision: 3,
        entryIds: ['b'],
      }),
      directoryDeltaPatch('pane-1', {
        type: 'entriesAdded',
        revision: 4,
        entries: [entry('c'), entry('b', 'B3')],
      }),
    );

    expect(updated.workspace.directories['request-1']?.entryIds).toEqual(['a', 'c', 'b']);
    expect(updated.workspace.directories['request-1']?.entriesById.a?.name).toBe('A2');
    expect(updated.workspace.directories['request-1']?.entriesById.b?.name).toBe('B3');
    expect(beforeDirectory?.entryIds).toEqual(['b', 'a']);
    expect(beforeDirectory?.entriesById.a?.name).toBe('a');
  });

  it('reduces runtime, operation, plugin, notification, and connection slices independently', () => {
    const plugin: PluginDescriptor = {
      id: 'plugin-1',
      name: 'Example',
      version: '1.0.0',
      description: 'Example plugin',
      enabled: true,
    };
    const patches = [
      runtimePatch({ kind: 'tauri' }),
      operationPatch(operation()),
      operationProgressPatch('operation-1', { completedItems: 2, completedBytes: 128 }),
      pluginPatch(plugin),
      notificationPatch({ id: 'notice-1', level: 'info', message: 'Done' }),
      connectionPatch({ status: 'open', lastEventId: 7 }),
    ] as const;
    const state = applyAppPatches(createInitialAppState('mock'), ...patches);

    expect(state.runtime.kind).toBe('tauri');
    expect(state.operations.byId['operation-1']?.progress.completedItems).toBe(2);
    expect(state.plugins.byId['plugin-1']).toEqual(plugin);
    expect(state.notifications.items).toHaveLength(1);
    expect(state.connection).toEqual({ status: 'open', lastEventId: 7 });
  });

  it('sets and deletes quick-filter drafts by tab key', () => {
    const withDraft = applyAppPatches(
      createInitialAppState('mock'),
      setQuickFilterDraftPatch('pane-1:tab-1', 'hello'),
    );
    expect(withDraft.quickFilterDrafts.byTabKey['pane-1:tab-1']).toBe('hello');

    const withSecond = applyAppPatches(
      withDraft,
      setQuickFilterDraftPatch('pane-1:tab-2', 'world'),
    );
    expect(withSecond.quickFilterDrafts.byTabKey['pane-1:tab-1']).toBe('hello');
    expect(withSecond.quickFilterDrafts.byTabKey['pane-1:tab-2']).toBe('world');

    const deleted = applyAppPatches(withSecond, deleteQuickFilterDraftPatch('pane-1:tab-1'));
    expect(deleted.quickFilterDrafts.byTabKey['pane-1:tab-1']).toBeUndefined();
    expect(deleted.quickFilterDrafts.byTabKey['pane-1:tab-2']).toBe('world');
  });

  it('initialises quick-filter drafts as empty', () => {
    const state = createInitialAppState('mock');
    expect(state.quickFilterDrafts.byTabKey).toEqual({});
  });

  it('sets and deletes closed-tab stacks by pane id', () => {
    const tab: TabProjection = {
      id: 'tab-1',
      title: 'My Tab',
      location: { providerId: 'local', uri: 'file:///tmp' },
      canNavigateBack: false,
      canNavigateForward: false,
      view: { showHidden: false, sort: [], quickFilter: null, columns: [], foldersFirst: true },
    };
    const withStack = applyAppPatches(
      createInitialAppState('mock'),
      setClosedTabStackPatch('pane-1', tab),
    );
    expect(withStack.closedTabStacks.byPaneId['pane-1']?.id).toBe('tab-1');

    const cleared = applyAppPatches(withStack, deleteClosedTabStackPatch('pane-1'));
    expect(cleared.closedTabStacks.byPaneId['pane-1']).toBeUndefined();
  });

  it('initialises closed-tab stacks as empty', () => {
    const state = createInitialAppState('mock');
    expect(state.closedTabStacks.byPaneId).toEqual({});
  });

  it('caches content matches by entry URI', () => {
    const matches = [{ lineNumber: 1, offset: 0, length: 5 }] as const;
    const withMatches = applyAppPatches(
      createInitialAppState('mock'),
      cacheContentMatchesPatch('file:///tmp/a.txt', matches),
    );
    expect(withMatches.contentMatches.byEntryUri['file:///tmp/a.txt']).toHaveLength(1);
    expect(withMatches.contentMatches.byEntryUri['file:///tmp/a.txt']?.[0]?.lineNumber).toBe(1);
  });

  it('initialises content matches as empty', () => {
    const state = createInitialAppState('mock');
    expect(state.contentMatches.byEntryUri).toEqual({});
  });
});
