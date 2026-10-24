import { describe, expect, it, vi } from 'vitest';

import type { FileManagerClient } from '../../api/client/file-manager-client';
import type { Location, SearchQuery, WorkspaceProjection } from '../../models';
import {
  createFindFilesController,
  type FindFilesControllerContext,
  searchQueryFromParams,
} from './find-files-controller';
import type { FindFilesSearchParams } from './find-files-dialog';
import { searchPresentation } from './search-presentation';

describe('searchQueryFromParams', () => {
  it('labels result scope from the submitted query, not the pane that later shows it', () => {
    const root = { providerId: 'local', uri: 'file:///current' };
    const params: FindFilesSearchParams = {
      mode: 'semantic',
      filenameQuery: '',
      semanticQuery: 'Mental stress',
      semanticScope: 'entireLibrary',
      contentRegex: false,
      contentCaseSensitive: false,
      contentWholeWord: false,
      recurse: true,
    };
    const libraryQuery = searchQueryFromParams(root, params, false);
    expect(
      searchPresentation(params, 'semantic', undefined, undefined, undefined, libraryQuery).scope,
    ).toEqual({ kind: 'allEnrolled' });
    const folderQuery = searchQueryFromParams(
      root,
      { ...params, semanticScope: 'currentFolder' },
      false,
    );
    expect(
      searchPresentation(params, 'semantic', undefined, undefined, undefined, folderQuery).scope,
    ).toEqual({ kind: 'folder', location: root });
  });

  it('uses the configured library for an unscoped concept-similarity search', () => {
    const root = { providerId: 'local', uri: 'file:///current' };
    const query = searchQueryFromParams(
      root,
      {
        mode: 'semantic',
        filenameQuery: '',
        semanticQuery: 'Machine learning. Learning from examples',
        semanticLibraryId: 'library-1',
        semanticScope: 'entireLibrary',
        contentRegex: false,
        contentCaseSensitive: false,
        contentWholeWord: false,
        recurse: true,
      },
      false,
    );
    expect(query.semantic).toEqual({
      query: 'Machine learning. Learning from examples',
      libraryId: 'library-1',
      scope: 'entireLibrary',
      enrolledRootIds: [],
    });
    expect(query.scope.locations).toEqual([root]);
  });

  it('preserves the selected concept identity as semantic-query provenance', () => {
    const query = searchQueryFromParams(
      { providerId: 'local', uri: 'file:///current' },
      {
        mode: 'semantic',
        filenameQuery: '',
        semanticQuery: 'Mental stress',
        semanticLibraryId: 'library-1',
        semanticScope: 'entireLibrary',
        selectedConcept: {
          vocabularyId: 'unesco-psychology',
          conceptUri: 'http://vocabularies.unesco.org/thesaurus/concept13013',
        },
        contentRegex: false,
        contentCaseSensitive: false,
        contentWholeWord: false,
        recurse: true,
      },
      false,
    );
    expect(query.mode).toBe('semantic');
    expect(query.semantic?.selectedConcept).toEqual({
      vocabularyId: 'unesco-psychology',
      conceptUri: 'http://vocabularies.unesco.org/thesaurus/concept13013',
    });
    expect(query.concept).toBeUndefined();
  });

  it('retains concept provenance only while the saved similarity text is unchanged', () => {
    const root = { providerId: 'local', uri: 'file:///current' };
    const params = {
      mode: 'semantic' as const,
      filenameQuery: '',
      semanticQuery: 'Mental stress',
      semanticLibraryId: 'library-1',
      semanticScope: 'entireLibrary' as const,
      contentRegex: false,
      contentCaseSensitive: false,
      contentWholeWord: false,
      recurse: true,
    };
    const existing = searchQueryFromParams(
      root,
      {
        ...params,
        selectedConcept: {
          vocabularyId: 'unesco-psychology',
          conceptUri: 'http://vocabularies.unesco.org/thesaurus/concept13013',
        },
      },
      false,
    );
    expect(searchQueryFromParams(root, params, false, existing).semantic?.selectedConcept).toEqual(
      existing.semantic?.selectedConcept,
    );
    expect(
      searchQueryFromParams(root, { ...params, semanticQuery: 'different text' }, false, existing)
        .semantic?.selectedConcept,
    ).toBeUndefined();
  });

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
        contentCaseSensitive: false,
        contentWholeWord: false,
        recurse: false,
        mimeTypes: ['text/*'],
        tags: ['work'],
      },
      false,
      existing,
    );

    expect(updated.scope).toEqual({ ...existing.scope, recurse: false });
    expect(updated.name).toEqual(existing.name);
    expect(updated.content).toEqual({
      ...existing.content,
      regex: true,
      caseSensitive: false,
      wholeWord: false,
    });
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
      contentCaseSensitive: false,
      contentWholeWord: false,
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
      navigateTo: vi.fn(),
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
        navigateTo: vi.fn(),
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

describe('FindFilesController branch view', () => {
  const root: Location = { providerId: 'local', uri: 'file:///Projects' };
  const resultLocation: Location = { providerId: 'search', uri: 'search://local/branch-1' };

  function setup(active: Location) {
    const roots = new Map<string, Location>();
    const queries = new Map<string, SearchQuery>();
    const presentations = new Map();
    const startSearch = vi.fn().mockResolvedValue({
      searchId: 'branch-1',
      location: resultLocation,
      limitations: [],
      executionMode: 'liveRecursive',
    });
    const cancelSearch = vi.fn().mockResolvedValue(undefined);
    let activeLocation = active;
    let generation = 0;
    const navigateTo = vi.fn();
    const context: FindFilesControllerContext = {
      getFindFilesOpen: () => false,
      getFindFilesRoot: () => undefined,
      getFindFilesSearchId: () => undefined,
      getFindFilesError: () => undefined,
      getFindFilesGeneration: () => generation,
      getFindFilesRootsByLocationUri: () => roots,
      getFindFilesPresentationsByLocationUri: () => presentations,
      getFindFilesParamsByLocationUri: () => new Map(),
      getFindFilesQueriesByLocationUri: () => queries,
      setFindFilesOpen: vi.fn(),
      setFindFilesRoot: vi.fn(),
      setFindFilesSearchId: vi.fn(),
      setFindFilesTargetPane: vi.fn(),
      clearFindFilesTargetPane: vi.fn(),
      setFindFilesSearchStartPending: vi.fn(),
      setFindFilesError: vi.fn(),
      setFindFilesGeneration: (next) => {
        generation = next;
      },
      getActiveDirectory: () => ({ paneId: 'pane-1', location: activeLocation }),
      getWorkspace: () =>
        ({
          id: 'workspace-1',
          activePaneId: 'pane-1',
          paneOrder: ['pane-1'],
          panesById: {
            'pane-1': {
              activeTabId: 'tab-1',
              tabsById: { 'tab-1': { view: { showHidden: true } } },
            },
          },
        }) as unknown as WorkspaceProjection,
      getClient: () => ({ startSearch, cancelSearch }) as unknown as FileManagerClient,
      getPaneLocationUri: () => activeLocation.uri,
      redraw: vi.fn(),
      openTabAt: vi.fn(),
      navigateTo,
      reportLimitations: vi.fn(),
    };
    return {
      context,
      startSearch,
      cancelSearch,
      navigateTo,
      roots,
      setActive: (location: Location) => {
        activeLocation = location;
      },
    };
  }

  it('starts an empty-name recursive files-only search of the active folder in the same pane', async () => {
    const { context, startSearch, roots } = setup(root);
    const controller = createFindFilesController(context);

    controller.toggleBranchView();

    await vi.waitFor(() => expect(startSearch).toHaveBeenCalledOnce());
    const request = startSearch.mock.calls[0]?.[0] as {
      query: string;
      roots: Location[];
      structuredQuery: SearchQuery;
    };
    expect(request.query).toBe('');
    expect(request.roots).toEqual([root]);
    expect(request.structuredQuery.scope).toEqual({
      locations: [root],
      recurse: true,
      showHidden: true,
    });
    expect(request.structuredQuery.entryKinds).toEqual(['file']);
    expect(request.structuredQuery.name).toBeUndefined();
    await vi.waitFor(() =>
      expect(context.setFindFilesTargetPane).toHaveBeenCalledWith('branch-1', 'pane-1'),
    );
    expect(roots.get(resultLocation.uri)).toEqual(root);
    expect(context.openTabAt).not.toHaveBeenCalled();
  });

  it('toggling again cancels the scan and restores the original folder', async () => {
    const { context, startSearch, cancelSearch, navigateTo, setActive } = setup(root);
    const controller = createFindFilesController(context);
    controller.toggleBranchView();
    await vi.waitFor(() => expect(context.setFindFilesTargetPane).toHaveBeenCalled());
    setActive(resultLocation);

    controller.toggleBranchView();

    expect(cancelSearch).toHaveBeenCalledWith('branch-1');
    expect(navigateTo).toHaveBeenCalledWith('pane-1', root);
    expect(startSearch).toHaveBeenCalledOnce();
  });

  it('does not turn an ordinary search result into a branch view', () => {
    const search: Location = { providerId: 'search', uri: 'search://local/other' };
    const { context, startSearch, navigateTo } = setup(search);

    createFindFilesController(context).toggleBranchView();

    expect(startSearch).not.toHaveBeenCalled();
    expect(navigateTo).not.toHaveBeenCalled();
  });
});
