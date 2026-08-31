import { describe, expect, it, vi } from 'vitest';

import type { FileManagerClient } from '../../api/client/file-manager-client';
import type { Location, SearchQuery, WorkspaceProjection } from '../../models';
import {
  createFindFilesController,
  type FindFilesControllerContext,
  searchQueryFromParams,
} from './find-files-controller';
import type { FindFilesSearchParams } from './find-files-dialog';

describe('searchQueryFromParams', () => {
  it('preserves unexposed query semantics and scope while editing', () => {
    const existing: SearchQuery = {
      schemaVersion: 1,
      scope: {
        locations: [{ providerId: 'sftp', uri: 'sftp://user@example.test/archive' }],
        recurse: true,
        showHidden: true,
      },
      name: { pattern: '*.md', mode: 'glob', caseSensitive: true },
      content: { query: 'TODO', regex: false, caseSensitive: true, wholeWord: true },
      entryKinds: ['file', 'directory'],
      mimeTypes: ['text/*'],
      gitStatuses: ['modified'],
      tags: ['work'],
      metadata: { owner: 'alice' },
    };

    const updated = searchQueryFromParams(
      { providerId: 'local', uri: 'file:///current' },
      {
        filenameQuery: '*.md',
        contentQuery: 'TODO',
        contentRegex: true,
        recurse: false,
        mimeTypes: ['text/*'],
        tags: ['work'],
      },
      false,
      existing,
    );

    expect(updated.scope).toEqual({ ...existing.scope, recurse: false });
    expect(updated.name).toEqual(existing.name);
    expect(updated.content).toEqual({ ...existing.content, regex: true });
    expect(updated.entryKinds).toEqual(existing.entryKinds);
    expect(updated.gitStatuses).toEqual(existing.gitStatuses);
    expect(updated.metadata).toEqual(existing.metadata);
  });
});

describe('FindFilesController refresh', () => {
  it('reruns the persisted multi-root request through the normal search lifecycle', async () => {
    const locationUri = 'search://local/old-search';
    const query: SearchQuery = {
      schemaVersion: 1,
      scope: {
        locations: [
          { providerId: 'local', uri: 'file:///one' },
          { providerId: 'local', uri: 'file:///two' },
        ],
        recurse: true,
        showHidden: true,
      },
      name: { pattern: 'report', mode: 'substring', caseSensitive: false },
      entryKinds: ['file'],
      mimeTypes: [],
      gitStatuses: [],
      tags: [],
      metadata: {},
    };
    const params: FindFilesSearchParams = {
      filenameQuery: 'report',
      contentRegex: false,
      recurse: true,
    };
    const startSearch = vi.fn().mockResolvedValue({
      searchId: 'new-search',
      location: { providerId: 'search', uri: 'search://local/new-search' },
      limitations: [],
      executionMode: 'indexed',
    });

    const cancelSearch = vi.fn().mockResolvedValue(undefined);
    const client = {
      startSearch,
      cancelSearch,
    } as unknown as FileManagerClient;
    const queries = new Map([[locationUri, query]]);
    const presentations = new Map([
      [
        locationUri,
        {
          kind: 'filename' as const,
          term: 'report',
          label: 'Quarterly reports',
          executionMode: 'liveRecursive' as const,
        },
      ],
    ]);
    const context: FindFilesControllerContext = {
      getFindFilesOpen: () => false,
      getFindFilesRoot: () => undefined,
      getFindFilesSearchId: () => 'unrelated-current-search',
      getFindFilesError: () => undefined,
      getFindFilesGeneration: () => 0,
      getFindFilesRootsByLocationUri: () => new Map<string, Location>(),
      getFindFilesPresentationsByLocationUri: () => presentations,
      getFindFilesParamsByLocationUri: () => new Map([[locationUri, params]]),
      getFindFilesQueriesByLocationUri: () => queries,
      setFindFilesOpen: vi.fn(),
      setFindFilesRoot: vi.fn(),
      setFindFilesSearchId: vi.fn(),
      setFindFilesTargetPane: vi.fn(),
      clearFindFilesTargetPane: vi.fn(),
      setFindFilesSearchStartPending: vi.fn(),
      setFindFilesError: vi.fn(),
      setFindFilesGeneration: vi.fn(),
      getActiveDirectory: () => ({
        paneId: 'pane-1',
        location: query.scope.locations[0] as Location,
      }),
      getWorkspace: () =>
        ({
          id: 'workspace-1',
          activePaneId: 'pane-2',
          paneOrder: ['pane-1', 'pane-2'],
        }) as unknown as WorkspaceProjection,
      getClient: () => client,
      getPaneLocationUri: () => locationUri,
      redraw: vi.fn(),
      openTabAt: vi.fn(),
      reportLimitations: vi.fn(),
    };

    createFindFilesController(context).refreshSearch(locationUri, 'pane-1');

    await vi.waitFor(() => expect(startSearch).toHaveBeenCalledOnce());
    expect(cancelSearch).toHaveBeenCalledWith('old-search');
    expect(cancelSearch).not.toHaveBeenCalledWith('unrelated-current-search');
    await vi.waitFor(() =>
      expect(context.setFindFilesTargetPane).toHaveBeenCalledWith('new-search', 'pane-1'),
    );
    expect(presentations.get('search://local/new-search')?.label).toBe('Quarterly reports');
    expect(context.setFindFilesOpen).not.toHaveBeenCalled();
    expect(context.setFindFilesRoot).not.toHaveBeenCalled();
    expect(startSearch).toHaveBeenCalledWith(
      expect.objectContaining({
        query: 'report',
        roots: query.scope.locations,
        structuredQuery: query,
        workspaceId: 'workspace-1',
      }),
    );
  });

  describe('FindFilesController saved searches', () => {
    it('opens a new result tab with its search root as durable history', async () => {
      const root = { providerId: 'local', uri: 'file:///Documents' };
      const resultLocation = {
        providerId: 'search',
        uri: 'search://local/new-search',
      };
      const query: SearchQuery = {
        schemaVersion: 1,
        scope: { locations: [root], recurse: true, showHidden: false },
        name: { pattern: '*.md', mode: 'glob', caseSensitive: false },
        entryKinds: ['file'],
        mimeTypes: [],
        gitStatuses: [],
        tags: [],
        metadata: {},
      };
      const openTabAt = vi.fn();
      let generation = 0;
      const context = {
        getFindFilesOpen: () => false,
        getFindFilesRoot: () => undefined,
        getFindFilesSearchId: () => undefined,
        getFindFilesError: () => undefined,
        getFindFilesGeneration: () => generation,
        getFindFilesRootsByLocationUri: () => new Map<string, Location>(),
        getFindFilesPresentationsByLocationUri: () => new Map(),
        getFindFilesParamsByLocationUri: () => new Map(),
        getFindFilesQueriesByLocationUri: () => new Map(),
        setFindFilesOpen: vi.fn(),
        setFindFilesRoot: vi.fn(),
        setFindFilesSearchId: vi.fn(),
        setFindFilesTargetPane: vi.fn(),
        clearFindFilesTargetPane: vi.fn(),
        setFindFilesSearchStartPending: vi.fn(),
        setFindFilesError: vi.fn(),
        setFindFilesGeneration: (next: number) => {
          generation = next;
        },
        getActiveDirectory: () => ({ paneId: 'pane-1', location: root }),
        getWorkspace: () =>
          ({
            id: 'workspace-1',
            activePaneId: 'pane-1',
            paneOrder: ['pane-1'],
          }) as unknown as WorkspaceProjection,
        getClient: () =>
          ({
            startSearch: vi.fn().mockResolvedValue({
              searchId: 'new-search',
              location: resultLocation,
              limitations: [],
              executionMode: 'indexed',
            }),
          }) as unknown as FileManagerClient,
        getPaneLocationUri: () => root.uri,
        redraw: vi.fn(),
        openTabAt,
        reportLimitations: vi.fn(),
      } satisfies FindFilesControllerContext;

      createFindFilesController(context).startSavedSearch(
        { id: 'saved-1', name: 'Documents', pinned: true, query },
        'newTab',
      );

      await vi.waitFor(() => expect(openTabAt).toHaveBeenCalled());
      expect(openTabAt).toHaveBeenCalledWith('pane-1', resultLocation, root);
    });
  });
});
