import { describe, expect, it, vi } from 'vitest';

import type {
  BackendEvent,
  Connection,
  Operation,
  OperationConflict,
  WorkspaceProjection,
} from '../../models';
import {
  type ChecksumState,
  type DuplicateState,
  initialChecksumState,
  initialDuplicateState,
  withChecksumJobStarted,
  withDuplicateScanStarted,
} from '../checksums/checksum-state';
import {
  type ComparisonState,
  initialComparisonState,
  withComparisonStarted,
} from '../comparison/comparison-state';
import { createOperationsState } from '../operations/operation-state';
import { type BackendEventContext, createBackendEventHandler } from './backend-event-handler';

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

type WorkspaceId = string & { readonly _brand: unique symbol };
type PaneId = string & { readonly _brand: unique symbol };
type ConnectionId = string & { readonly _brand: unique symbol };

const WS_ID = 'ws-1' as WorkspaceId;
const PANE_ID = 'pane-1' as PaneId;

function makeEvent(payload: BackendEvent['payload'], workspaceId?: string): BackendEvent {
  return {
    eventId: 1,
    timestamp: '2026-01-01T00:00:00Z',
    ...(workspaceId !== undefined ? { workspaceId: workspaceId as WorkspaceId } : {}),
    payload,
  };
}

function makeContext(overrides: Partial<BackendEventContext> = {}): BackendEventContext {
  let comparisonState = initialComparisonState();
  let checksumState = initialChecksumState();
  let duplicateState = initialDuplicateState();
  return {
    getComparisonState: vi.fn(() => comparisonState),
    setComparisonState: vi.fn((next: ComparisonState) => {
      comparisonState = next;
    }),
    markComparisonDifferences: vi.fn(),
    getChecksumState: vi.fn(() => checksumState),
    setChecksumState: vi.fn((next: ChecksumState) => {
      checksumState = next;
    }),
    getDuplicateState: vi.fn(() => duplicateState),
    setDuplicateState: vi.fn((next: DuplicateState) => {
      duplicateState = next;
    }),
    getWorkspaceId: vi.fn(() => WS_ID),
    getWorkspaceRevision: vi.fn(() => 5),
    replaceWorkspace: vi.fn(),
    refreshWorkspaceSummaries: vi.fn(),
    setWorkspaceSummaries: vi.fn(),
    setWorkspaceActionError: vi.fn(),
    recoverActiveWorkspace: vi.fn(() => Promise.resolve()),
    listWorkspaces: vi.fn(() => Promise.resolve([])),
    getWorkspace: vi.fn(() => Promise.resolve({} as WorkspaceProjection)),
    setPendingConflict: vi.fn(),
    getPendingOperationEvents: vi.fn(() => []),
    pushPendingOperationEvent: vi.fn(),
    clearPendingOperationEvents: vi.fn(() => []),
    getOperationFrame: vi.fn(() => undefined),
    setOperationFrame: vi.fn(),
    getOperations: vi.fn(() => createOperationsState()),
    setOperations: vi.fn(),
    getDismissedOperationIds: vi.fn(() => new Set<string>()),
    clearDismissedOperation: vi.fn(),
    scheduleAutoDismiss: vi.fn(),
    removeOperationSourcesFromSearchResults: vi.fn(),
    removeOperationSourcesFromDiskUsage: vi.fn(),
    getActiveDirectoryRevision: vi.fn(() => undefined),
    applyDelta: vi.fn(),
    refetchAffectedPanes: vi.fn(),
    getPlugins: vi.fn(() => []),
    setPlugins: vi.fn(),
    listPlugins: vi.fn(() => Promise.resolve([])),
    getCurrentIconThemeSetting: vi.fn(() => undefined),
    applyIconTheme: vi.fn(),
    getConnections: vi.fn(() => []),
    setConnections: vi.fn(),
    getConnection: vi.fn(() => Promise.resolve({} as Connection)),
    getFindFilesSearchId: vi.fn(() => undefined),
    setSearchExecutionMode: vi.fn(),
    cacheContentMatches: vi.fn(),
    findPanesWithUri: vi.fn(() => []),
    revealSearchResults: vi.fn(() => Promise.resolve([])),
    loadPane: vi.fn(() => Promise.resolve()),
    reportSearchCompletion: vi.fn(),
    reportSearchWithoutResults: vi.fn(),
    applyDiskUsageProgress: vi.fn(),
    applyDiskUsageFinalizing: vi.fn(),
    applyDiskUsageFailure: vi.fn(),
    redraw: vi.fn(),
    ...overrides,
  };
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe('createBackendEventHandler', () => {
  describe('diskUsage.progress', () => {
    it('applies the matching progressive tree and redraws', () => {
      const ctx = makeContext();
      const handler = createBackendEventHandler(ctx);
      const result = {
        root: {
          name: 'home',
          location: { providerId: 'local', uri: 'file:///home' },
          kind: 'directory' as const,
          logicalBytes: 10,
          physicalBytes: 10,
          collapsed: false,
          children: [],
        },
        unreadableEntries: 0,
        unreadable: [],
        scannedEntries: 17,
      };

      handler(
        makeEvent(
          {
            type: 'diskUsage.progress',
            scanId: 'scan-1',
            root: result.root,
            unreadableEntries: result.unreadableEntries,
            unreadable: result.unreadable,
            scannedEntries: result.scannedEntries,
            isComplete: false,
          },
          WS_ID,
        ),
      );

      expect(ctx.applyDiskUsageProgress).toHaveBeenCalledWith('scan-1', result, false);
      expect(ctx.redraw).toHaveBeenCalled();
    });

    describe('diskUsage.failed', () => {
      it('stops the matching scan with the backend message', () => {
        const ctx = makeContext();
        const handler = createBackendEventHandler(ctx);

        handler(
          makeEvent(
            {
              type: 'diskUsage.failed',
              scanId: 'scan-1',
              code: 'internal',
              message: 'Scanner worker stopped unexpectedly',
            },
            WS_ID,
          ),
        );

        expect(ctx.applyDiskUsageFailure).toHaveBeenCalledWith(
          'scan-1',
          'Scanner worker stopped unexpectedly',
        );
        expect(ctx.redraw).toHaveBeenCalled();
      });

      describe('diskUsage.finalizing', () => {
        it('marks traversal as complete while the result tree is assembled', () => {
          const ctx = makeContext();
          const handler = createBackendEventHandler(ctx);

          handler(
            makeEvent(
              {
                type: 'diskUsage.finalizing',
                scanId: 'scan-1',
                scannedEntries: 4_302_322,
              },
              WS_ID,
            ),
          );

          expect(ctx.applyDiskUsageFinalizing).toHaveBeenCalledWith('scan-1', 4_302_322);
          expect(ctx.redraw).toHaveBeenCalled();
        });
      });
    });
  });

  describe('operation.conflict', () => {
    it('sets the pending conflict and redraws', () => {
      const ctx = makeContext();
      const handler = createBackendEventHandler(ctx);
      const conflict: OperationConflict = {
        operationId: 'op-1' as BackendEvent['payload'] extends { operationId: infer T } ? T : never,
        conflictId: 'c-1',
        message: 'File already exists',
        source: { name: 'a.txt', kind: 'file' },
        destination: { name: 'a.txt', kind: 'file' },
      };
      handler(makeEvent({ type: 'operation.conflict', ...conflict }, WS_ID));

      expect(ctx.setPendingConflict).toHaveBeenCalledWith(
        expect.objectContaining({ type: 'operation.conflict', conflictId: 'c-1' }),
      );
      expect(ctx.redraw).toHaveBeenCalled();
    });

    it('ignores the event when it belongs to a different workspace', () => {
      const ctx = makeContext();
      const handler = createBackendEventHandler(ctx);
      const conflict: OperationConflict = {
        operationId: 'op-1' as never,
        conflictId: 'c-1',
        message: 'File already exists',
        source: { name: 'a.txt', kind: 'file' },
        destination: { name: 'a.txt', kind: 'file' },
      };
      handler(makeEvent({ type: 'operation.conflict', ...conflict }, 'other-ws'));

      expect(ctx.setPendingConflict).not.toHaveBeenCalled();
    });
  });

  describe('directory.snapshot', () => {
    it('applies a delta reset when the incoming revision is newer', () => {
      const ctx = makeContext({
        getActiveDirectoryRevision: vi.fn(() => 2),
      });
      const handler = createBackendEventHandler(ctx);

      handler(
        makeEvent(
          {
            type: 'directory.snapshot',
            snapshot: {
              paneId: PANE_ID,
              revision: 3,
              location: { providerId: 'local', uri: 'file:///tmp' },
              entries: [],
              writable: true,
              hasMore: false,
              requestId: 'r1',
              loadingState: { type: 'loaded' },
            },
          },
          WS_ID,
        ),
      );

      expect(ctx.applyDelta).toHaveBeenCalledWith(
        PANE_ID,
        expect.objectContaining({ type: 'reset' }),
      );
    });

    it('skips the snapshot when the revision is not newer', () => {
      const ctx = makeContext({
        getActiveDirectoryRevision: vi.fn(() => 5),
      });
      const handler = createBackendEventHandler(ctx);

      handler(
        makeEvent(
          {
            type: 'directory.snapshot',
            snapshot: {
              paneId: PANE_ID,
              revision: 5,
              location: { providerId: 'local', uri: 'file:///tmp' },
              entries: [],
              writable: true,
              hasMore: false,
              requestId: 'r1',
              loadingState: { type: 'loaded' },
            },
          },
          WS_ID,
        ),
      );

      expect(ctx.applyDelta).not.toHaveBeenCalled();
    });
  });

  describe('plugin.changed', () => {
    it('merges the summary into the existing plugin list and redraws', () => {
      const existing = [
        { id: 'plug-a', name: 'Plug A', version: '1.0.0', enabled: true, description: '' },
      ];
      const ctx = makeContext({ getPlugins: vi.fn(() => existing) });
      const handler = createBackendEventHandler(ctx);

      handler(
        makeEvent(
          {
            type: 'plugin.changed',
            plugin: { id: 'plug-a', name: 'Plug A', version: '1.1.0', enabled: false },
          },
          WS_ID,
        ),
      );

      expect(ctx.setPlugins).toHaveBeenCalledWith([
        expect.objectContaining({ id: 'plug-a', version: '1.1.0', enabled: false }),
      ]);
      expect(ctx.redraw).toHaveBeenCalled();
    });

    it('re-fetches the full plugin list and re-applies the icon theme', async () => {
      const listed = [
        { id: 'plug-a', name: 'Plug A', version: '1.1.0', enabled: true, description: '' },
      ];
      const ctx = makeContext({
        getPlugins: vi.fn(() => []),
        listPlugins: vi.fn(() => Promise.resolve(listed)),
        getCurrentIconThemeSetting: vi.fn(() => 'plug-a'),
      });
      const handler = createBackendEventHandler(ctx);

      handler(
        makeEvent(
          {
            type: 'plugin.changed',
            plugin: { id: 'plug-a', name: 'Plug A', version: '1.1.0', enabled: true },
          },
          WS_ID,
        ),
      );
      // Allow listPlugins promise to settle.
      await new Promise((resolve) => setTimeout(resolve, 0));

      expect(ctx.setPlugins).toHaveBeenCalledWith(listed);
      expect(ctx.applyIconTheme).toHaveBeenCalledWith('plug-a');
    });
  });

  describe('connection.deleted', () => {
    it('removes the connection from the list and redraws', () => {
      const conn = { id: 'conn-1', name: 'My Server' } as unknown as Connection;
      const ctx = makeContext({ getConnections: vi.fn(() => [conn]) });
      const handler = createBackendEventHandler(ctx);

      handler(
        makeEvent({ type: 'connection.deleted', connectionId: 'conn-1' as ConnectionId }, WS_ID),
      );

      expect(ctx.setConnections).toHaveBeenCalledWith(
        expect.not.arrayContaining([expect.objectContaining({ id: 'conn-1' })]),
      );
      expect(ctx.redraw).toHaveBeenCalled();
    });
  });

  describe('search.resultsBatch', () => {
    it('caches content matches, triggers a reload, and reports completion for the matching pane', async () => {
      const searchId = 'search-42';
      const ctx = makeContext({
        getFindFilesSearchId: vi.fn(() => searchId),
        findPanesWithUri: vi.fn(() => [PANE_ID]),
        loadPane: vi.fn(() => Promise.resolve()),
      });
      const handler = createBackendEventHandler(ctx);

      handler(
        makeEvent(
          {
            type: 'search.resultsBatch',
            searchId,
            entries: [
              {
                id: 'entry-1' as never,
                name: 'result.ts',
                kind: 'file',
                location: { providerId: 'local', uri: 'file:///tmp/result.ts' },
                hidden: false,
                readOnly: false,
                metadataRevision: 0,
                contentMatches: [{ lineNumber: 1, offset: 0, length: 3 }],
              },
            ],
            isComplete: true,
            warningsCount: 0,
            executionMode: 'indexed',
          },
          WS_ID,
        ),
      );

      expect(ctx.cacheContentMatches).toHaveBeenCalledWith('file:///tmp/result.ts', [
        { lineNumber: 1, offset: 0, length: 3 },
      ]);
      expect(ctx.setSearchExecutionMode).toHaveBeenCalledWith(
        'search://local/search-42',
        'indexed',
      );
      await Promise.resolve();
      expect(ctx.loadPane).toHaveBeenCalledWith(PANE_ID, { background: false });
      expect(ctx.redraw).toHaveBeenCalled();

      // `reportSearchCompletion` must only run after the reload settles, not before - the
      // pane's listing needs to actually reflect the streamed-in results before anyone can
      // tell whether the search came up empty.
      await Promise.resolve();
      await Promise.resolve();
      expect(ctx.reportSearchCompletion).toHaveBeenCalledWith(PANE_ID, searchId);
    });

    it('does not report completion for a batch that is not yet complete', async () => {
      const searchId = 'search-42';
      const ctx = makeContext({
        getFindFilesSearchId: vi.fn(() => searchId),
        findPanesWithUri: vi.fn(() => [PANE_ID]),
      });
      const handler = createBackendEventHandler(ctx);

      handler(
        makeEvent(
          {
            type: 'search.resultsBatch',
            searchId,
            entries: [],
            isComplete: false,
            warningsCount: 0,
            executionMode: 'liveRecursive',
          },
          WS_ID,
        ),
      );

      await Promise.resolve();
      await Promise.resolve();
      expect(ctx.reportSearchCompletion).not.toHaveBeenCalled();
    });

    it('opens the results pane only after the first non-empty batch', async () => {
      const searchId = 'search-42';
      const ctx = makeContext({
        getFindFilesSearchId: vi.fn(() => searchId),
        revealSearchResults: vi.fn(() => Promise.resolve([PANE_ID])),
      });
      const handler = createBackendEventHandler(ctx);

      handler(
        makeEvent(
          {
            type: 'search.resultsBatch',
            searchId,
            entries: [],
            isComplete: false,
            warningsCount: 0,
            executionMode: 'liveRecursive',
          },
          WS_ID,
        ),
      );
      expect(ctx.revealSearchResults).not.toHaveBeenCalled();

      handler(
        makeEvent(
          {
            type: 'search.resultsBatch',
            searchId,
            entries: [
              {
                id: 'entry-1' as never,
                name: 'result.ts',
                kind: 'file',
                location: { providerId: 'local', uri: 'file:///tmp/result.ts' },
                hidden: false,
                readOnly: false,
                metadataRevision: 0,
              },
            ],
            isComplete: false,
            warningsCount: 0,
            executionMode: 'liveRecursive',
          },
          WS_ID,
        ),
      );
      await Promise.resolve();
      expect(ctx.revealSearchResults).toHaveBeenCalledWith(searchId);
    });

    it('reports an empty completed search without opening a results pane', () => {
      const searchId = 'search-42';
      const ctx = makeContext({
        getFindFilesSearchId: vi.fn(() => searchId),
      });
      const handler = createBackendEventHandler(ctx);

      handler(
        makeEvent(
          {
            type: 'search.resultsBatch',
            searchId,
            entries: [],
            isComplete: true,
            warningsCount: 0,
            executionMode: 'liveRecursive',
          },
          WS_ID,
        ),
      );

      expect(ctx.reportSearchWithoutResults).toHaveBeenCalledWith(searchId);
      expect(ctx.revealSearchResults).not.toHaveBeenCalled();
    });

    it('runs a trailing reload when another batch arrives during an in-flight reload', async () => {
      const searchId = 'search-42';
      let resolveFirst!: () => void;
      const firstLoad = new Promise<void>((resolve) => {
        resolveFirst = resolve;
      });
      const ctx = makeContext({
        getFindFilesSearchId: vi.fn(() => searchId),
        findPanesWithUri: vi.fn(() => [PANE_ID]),
        loadPane: vi.fn().mockReturnValueOnce(firstLoad).mockResolvedValue(undefined),
      });
      const handler = createBackendEventHandler(ctx);
      const batch = makeEvent(
        {
          type: 'search.resultsBatch',
          searchId,
          entries: [
            {
              id: 'entry-1' as never,
              name: 'result.ts',
              kind: 'file',
              location: { providerId: 'local', uri: 'file:///tmp/result.ts' },
              hidden: false,
              readOnly: false,
              metadataRevision: 0,
            },
          ],
          isComplete: false,
          warningsCount: 0,
          executionMode: 'liveRecursive',
        },
        WS_ID,
      );

      handler(batch);
      await Promise.resolve();
      handler(batch);
      await Promise.resolve();
      expect(ctx.loadPane).toHaveBeenCalledTimes(1);

      resolveFirst();
      await Promise.resolve();
      await Promise.resolve();
      expect(ctx.loadPane).toHaveBeenCalledTimes(2);
    });

    it('retains completion when the final empty batch arrives while the results pane is opening', async () => {
      const searchId = 'search-42';
      let resolveReveal!: (paneIds: readonly PaneId[]) => void;
      const reveal = new Promise<readonly PaneId[]>((resolve) => {
        resolveReveal = resolve;
      });
      const ctx = makeContext({
        getFindFilesSearchId: vi.fn(() => searchId),
        revealSearchResults: vi.fn(() => reveal),
      });
      const handler = createBackendEventHandler(ctx);

      handler(
        makeEvent(
          {
            type: 'search.resultsBatch',
            searchId,
            entries: [
              {
                id: 'entry-1' as never,
                name: 'result.ts',
                kind: 'file',
                location: { providerId: 'local', uri: 'file:///tmp/result.ts' },
                hidden: false,
                readOnly: false,
                metadataRevision: 0,
              },
            ],
            isComplete: false,
            warningsCount: 0,
            executionMode: 'liveRecursive',
          },
          WS_ID,
        ),
      );
      handler(
        makeEvent(
          {
            type: 'search.resultsBatch',
            searchId,
            entries: [],
            isComplete: true,
            warningsCount: 0,
            executionMode: 'liveRecursive',
          },
          WS_ID,
        ),
      );

      resolveReveal([PANE_ID]);
      await vi.waitFor(() => {
        expect(ctx.reportSearchCompletion).toHaveBeenCalledWith(PANE_ID, searchId);
      });

      expect(ctx.revealSearchResults).toHaveBeenCalledOnce();
      expect(ctx.loadPane).toHaveBeenLastCalledWith(PANE_ID, { background: false });
    });

    it('ignores batches belonging to a different search', () => {
      const ctx = makeContext({
        getFindFilesSearchId: vi.fn(() => 'search-other'),
      });
      const handler = createBackendEventHandler(ctx);

      handler(
        makeEvent(
          {
            type: 'search.resultsBatch',
            searchId: 'search-42',
            entries: [],
            isComplete: true,
            warningsCount: 0,
            executionMode: 'liveRecursive',
          },
          WS_ID,
        ),
      );

      expect(ctx.cacheContentMatches).not.toHaveBeenCalled();
      expect(ctx.loadPane).not.toHaveBeenCalled();
    });
  });

  describe('comparison.resultsBatch', () => {
    it('merges the batch into comparison state and redraws', () => {
      const ctx = makeContext();
      const handler = createBackendEventHandler(ctx);

      handler(
        makeEvent(
          {
            type: 'comparison.resultsBatch',
            comparisonId: 'comparison-1',
            entries: [{ relativePath: 'a.txt', status: 'onlyLeft' }],
            isComplete: true,
            warningsCount: 0,
          },
          WS_ID,
        ),
      );

      expect(ctx.setComparisonState).toHaveBeenCalledOnce();
      expect(ctx.getComparisonState().statusByRelativePath.size).toBe(0);
      expect(ctx.redraw).toHaveBeenCalled();
    });

    it('is a no-op for a batch belonging to a comparison no longer tracked', () => {
      const ctx = makeContext();
      const handler = createBackendEventHandler(ctx);

      handler(
        makeEvent(
          {
            type: 'comparison.resultsBatch',
            comparisonId: 'stale-comparison',
            entries: [{ relativePath: 'a.txt', status: 'onlyLeft' }],
            isComplete: true,
            warningsCount: 0,
          },
          WS_ID,
        ),
      );

      expect(ctx.setComparisonState).toHaveBeenCalledOnce();
      expect(ctx.redraw).toHaveBeenCalled();
      // The batch itself was discarded (stale id) and the untouched current state isn't
      // complete, so nothing should be marked selected.
      expect(ctx.markComparisonDifferences).not.toHaveBeenCalled();
    });

    it('applies a checksum results batch to the tracked job (task 0077)', () => {
      const ctx = makeContext();
      ctx.setChecksumState(withChecksumJobStarted('job-1', ['sha256'], 1));
      createBackendEventHandler(ctx)(
        makeEvent(
          {
            type: 'checksum.resultsBatch',
            jobId: 'job-1',
            entries: [
              {
                location: { providerId: 'local', uri: 'file:///a.txt' },
                relativePath: 'a.txt',
                size: 3,
                checksums: { sha256: 'aa' },
              },
            ],
            isComplete: true,
            isCancelled: false,
          },
          WS_ID,
        ),
      );
      expect(ctx.getChecksumState().entries).toHaveLength(1);
      expect(ctx.getChecksumState().isComplete).toBe(true);
    });

    it('ignores a checksum batch for an untracked job (task 0077)', () => {
      const ctx = makeContext();
      createBackendEventHandler(ctx)(
        makeEvent(
          {
            type: 'checksum.resultsBatch',
            jobId: 'stale',
            entries: [],
            isComplete: true,
            isCancelled: false,
          },
          WS_ID,
        ),
      );
      expect(ctx.getChecksumState().jobId).toBeUndefined();
      expect(ctx.getChecksumState().isComplete).toBe(false);
    });

    it('applies duplicate results and preserves the cancelled flag (task 0077)', () => {
      const ctx = makeContext();
      ctx.setDuplicateState(
        withDuplicateScanStarted('scan-1', [{ providerId: 'local', uri: 'file:///root' }]),
      );
      createBackendEventHandler(ctx)(
        makeEvent(
          {
            type: 'duplicates.resultsReady',
            scanId: 'scan-1',
            groups: [],
            isCancelled: true,
            warningsCount: 2,
          },
          WS_ID,
        ),
      );
      expect(ctx.getDuplicateState().isComplete).toBe(true);
      expect(ctx.getDuplicateState().isCancelled).toBe(true);
      expect(ctx.getDuplicateState().warningsCount).toBe(2);
    });

    it('marks differing entries selected once the tracked comparison completes', () => {
      let comparisonState = withComparisonStarted({
        comparisonId: 'comparison-1',
        criteria: 'sizeAndTimestamp',
        leftRoot: { providerId: 'local', uri: 'file:///left' },
        rightRoot: { providerId: 'local', uri: 'file:///right' },
        leftPaneId: PANE_ID,
        rightPaneId: 'pane-2' as PaneId,
      });
      const ctx = makeContext({
        getComparisonState: vi.fn(() => comparisonState),
        setComparisonState: vi.fn((next: ComparisonState) => {
          comparisonState = next;
        }),
      });
      const handler = createBackendEventHandler(ctx);

      handler(
        makeEvent(
          {
            type: 'comparison.resultsBatch',
            comparisonId: 'comparison-1',
            entries: [{ relativePath: 'a.txt', status: 'onlyLeft' }],
            isComplete: true,
            warningsCount: 0,
          },
          WS_ID,
        ),
      );

      expect(ctx.markComparisonDifferences).toHaveBeenCalledOnce();
      expect(ctx.markComparisonDifferences).toHaveBeenCalledWith(
        expect.objectContaining({ isComplete: true }),
      );
    });

    it('does not mark differences while the comparison is still streaming', () => {
      let comparisonState = withComparisonStarted({
        comparisonId: 'comparison-1',
        criteria: 'sizeAndTimestamp',
        leftRoot: { providerId: 'local', uri: 'file:///left' },
        rightRoot: { providerId: 'local', uri: 'file:///right' },
        leftPaneId: PANE_ID,
        rightPaneId: 'pane-2' as PaneId,
      });
      const ctx = makeContext({
        getComparisonState: vi.fn(() => comparisonState),
        setComparisonState: vi.fn((next: ComparisonState) => {
          comparisonState = next;
        }),
      });
      const handler = createBackendEventHandler(ctx);

      handler(
        makeEvent(
          {
            type: 'comparison.resultsBatch',
            comparisonId: 'comparison-1',
            entries: [{ relativePath: 'a.txt', status: 'onlyLeft' }],
            isComplete: false,
            warningsCount: 0,
          },
          WS_ID,
        ),
      );

      expect(ctx.markComparisonDifferences).not.toHaveBeenCalled();
    });
  });

  describe('workspace.deleted (active workspace)', () => {
    it('lists workspaces and triggers recovery when the active workspace is deleted', async () => {
      const summaries = [
        {
          id: 'ws-2' as WorkspaceId,
          name: 'Other',
          revision: 1,
          ephemeral: false,
          updatedAt: '2026-01-01T00:00:00Z',
        },
      ];
      const ctx = makeContext({
        listWorkspaces: vi.fn(() => Promise.resolve(summaries)),
        recoverActiveWorkspace: vi.fn(() => Promise.resolve()),
      });
      const handler = createBackendEventHandler(ctx);

      handler(makeEvent({ type: 'workspace.deleted', revision: 6 }, WS_ID));
      // Allow the full promise chain (listWorkspaces → then → recoverActiveWorkspace → finally) to settle.
      await new Promise((resolve) => setTimeout(resolve, 0));

      expect(ctx.setWorkspaceSummaries).toHaveBeenCalledWith(summaries);
      expect(ctx.recoverActiveWorkspace).toHaveBeenCalledWith(summaries);
      expect(ctx.redraw).toHaveBeenCalled();
    });

    it('only refreshes the summary list when a different workspace is deleted', () => {
      const ctx = makeContext();
      const handler = createBackendEventHandler(ctx);

      handler(makeEvent({ type: 'workspace.deleted', revision: 6 }, 'other-ws'));

      expect(ctx.refreshWorkspaceSummaries).toHaveBeenCalled();
      expect(ctx.listWorkspaces).not.toHaveBeenCalled();
    });
  });

  describe('terminal operations', () => {
    it('forces a foreground pane refetch when a mutating operation reaches a terminal state', () => {
      const previous = createOperationsState([
        {
          id: 'op-1' as never,
          kind: 'copy',
          state: 'running',
          sources: [],
          progress: { completedItems: 0, completedBytes: 0 },
          conflictPolicy: 'ask',
          createdAt: '2026-01-01T00:00:00Z',
        },
      ]);
      const completedOperation: Operation = {
        id: 'op-1' as never,
        kind: 'copy',
        state: 'completed',
        sources: [],
        progress: { completedItems: 1, completedBytes: 1 },
        conflictPolicy: 'ask',
        createdAt: '2026-01-01T00:00:00Z',
      };
      const pending: BackendEvent[] = [];
      const ctx = makeContext({
        getOperations: vi.fn(() => previous),
        getOperationFrame: vi.fn(() => undefined),
        pushPendingOperationEvent: vi.fn((event: BackendEvent) => {
          pending.push(event);
        }),
        clearPendingOperationEvents: vi.fn(() => {
          const events = [...pending];
          pending.length = 0;
          return events;
        }),
      });
      // Replace RAF with immediate execution for deterministic unit behavior.
      const raf = vi.spyOn(globalThis, 'requestAnimationFrame').mockImplementation((callback) => {
        callback(0);
        return 1;
      });
      const handler = createBackendEventHandler(ctx);

      try {
        handler(makeEvent({ type: 'operation.created', operation: completedOperation }, WS_ID));
      } finally {
        raf.mockRestore();
      }

      expect(ctx.refetchAffectedPanes).toHaveBeenCalledWith(undefined, { background: false });
    });

    it('removes successfully trashed sources from open search result snapshots', () => {
      const completedOperation: Operation = {
        id: 'op-1' as never,
        kind: 'trash',
        state: 'completed',
        sources: [
          {
            id: 'entry-1' as never,
            location: { providerId: 'local', uri: 'file:///tmp/report.pdf' },
          },
        ],
        progress: { completedItems: 1, completedBytes: 1 },
        conflictPolicy: 'ask',
        createdAt: '2026-01-01T00:00:00Z',
      };
      const pending: BackendEvent[] = [];
      const ctx = makeContext({
        pushPendingOperationEvent: vi.fn((event: BackendEvent) => pending.push(event)),
        clearPendingOperationEvents: vi.fn(() => pending.splice(0)),
      });
      const raf = vi.spyOn(globalThis, 'requestAnimationFrame').mockImplementation((callback) => {
        callback(0);
        return 1;
      });

      try {
        createBackendEventHandler(ctx)(
          makeEvent({ type: 'operation.completed', operation: completedOperation }, WS_ID),
        );
      } finally {
        raf.mockRestore();
      }

      expect(ctx.removeOperationSourcesFromSearchResults).toHaveBeenCalledExactlyOnceWith(
        completedOperation,
      );
      expect(ctx.removeOperationSourcesFromDiskUsage).toHaveBeenCalledExactlyOnceWith(
        completedOperation,
      );
    });

    it('does not auto-dismiss a completed operation while undo remains available', () => {
      const runningOperation: Operation = {
        id: 'undoable-trash' as never,
        kind: 'trash',
        state: 'running',
        sources: [],
        progress: { completedItems: 0, completedBytes: 0 },
        conflictPolicy: 'ask',
        createdAt: '2026-01-01T00:00:00Z',
      };
      const completedOperation: Operation = {
        ...runningOperation,
        state: 'completed',
        undo: { available: true },
      };
      const pending: BackendEvent[] = [];
      const ctx = makeContext({
        getOperations: vi.fn(() => createOperationsState([runningOperation])),
        pushPendingOperationEvent: vi.fn((event: BackendEvent) => pending.push(event)),
        clearPendingOperationEvents: vi.fn(() => pending.splice(0)),
      });
      const raf = vi.spyOn(globalThis, 'requestAnimationFrame').mockImplementation((callback) => {
        callback(0);
        return 1;
      });

      try {
        createBackendEventHandler(ctx)(
          makeEvent({ type: 'operation.completed', operation: completedOperation }, WS_ID),
        );
      } finally {
        raf.mockRestore();
      }

      expect(ctx.scheduleAutoDismiss).not.toHaveBeenCalled();
      expect(ctx.setOperations).toHaveBeenCalledWith(
        expect.objectContaining({
          byId: expect.objectContaining({ 'undoable-trash': completedOperation }),
        }),
      );
    });
  });
});
