import type { FileManagerClient } from '../../api/client/file-manager-client';
import { t } from '../../i18n';
import type { Location, PaneId, SavedSearch, SearchQuery, WorkspaceProjection } from '../../models';
import type { FindFilesSearchParams } from './find-files-dialog';
import { type SearchPresentation, searchPresentation } from './search-presentation';

/** Context required by FindFilesController for state access and dependencies. */
export interface FindFilesControllerContext {
  // State getters
  getFindFilesOpen(): boolean;
  getFindFilesRoot(): Location | undefined;
  getFindFilesSearchId(): string | undefined;
  getFindFilesError(): string | undefined;
  getFindFilesGeneration(): number;
  getFindFilesRootsByLocationUri(): Map<string, Location>;
  getFindFilesPresentationsByLocationUri(): Map<string, SearchPresentation>;
  getFindFilesParamsByLocationUri(): Map<string, FindFilesSearchParams>;
  getFindFilesQueriesByLocationUri(): Map<string, SearchQuery>;
  /** Captures a batch mode that arrived before startSearch resolved. */
  getSearchExecutionMode?(searchId: string): SearchPresentation['executionMode'] | undefined;

  // State setters
  setFindFilesOpen(open: boolean): void;
  setFindFilesRoot(root: Location | undefined): void;
  setFindFilesSearchId(searchId: string | undefined): void;
  setFindFilesTargetPane(searchId: string, paneId: PaneId): void;
  clearFindFilesTargetPane(searchId: string): void;
  setFindFilesSearchStartPending(pending: boolean): void;
  setFindFilesError(error: string | undefined): void;
  setFindFilesGeneration(generation: number): void;

  // Dependencies
  getActiveDirectory(): { paneId: PaneId; location: Location } | undefined;
  getWorkspace(): WorkspaceProjection | undefined;
  getClient(): FileManagerClient;
  getPaneLocationUri(paneId: PaneId): string | undefined;
  redraw(): void;
  openTabAt(paneId: PaneId, location: Location, historyOrigin?: Location): void;
  reportLimitations(message: string): void;
}

/** Controller interface for find-files operations. */
export interface FindFilesController {
  /**
   * Opens the filename search dialog at the active directory,
   * or reopens at the previous search's root if a search location is active.
   */
  openFindFiles(): void;

  /**
   * Closes the search dialog and cancels any in-flight search.
   */
  closeFindFiles(): void;

  /**
   * Closes the search dialog without cancelling the search now displayed in the active pane.
   */
  dismissFindFiles(): void;

  /**
   * Starts (or restarts) a search rooted at the dialog's current directory.
   */
  startFindFilesSearch(params: FindFilesSearchParams): void;
  startSavedSearch(saved: SavedSearch, target: SavedSearchOpenTarget): void;
  /** Starts the same request again, producing a fresh indexed/live result set. */
  refreshSearch(searchLocationUri: string, paneId: PaneId): void;

  /**
   * The active tab's current "show hidden files" setting, so a new search respects it.
   */
  activeShowHidden(paneId: PaneId): boolean;
}

export type SavedSearchOpenTarget = 'currentPane' | 'otherPane' | 'newTab';

export function searchQueryFromParams(
  root: Location,
  params: FindFilesSearchParams,
  showHidden: boolean,
  existing?: SearchQuery,
): SearchQuery {
  const name =
    params.filenameQuery.length === 0
      ? undefined
      : existing?.name?.pattern === params.filenameQuery
        ? existing.name
        : {
            pattern: params.filenameQuery,
            mode:
              params.filenameQuery.includes('*') || params.filenameQuery.includes('?')
                ? ('glob' as const)
                : ('substring' as const),
            caseSensitive: false,
          };
  const content =
    params.contentQuery === undefined
      ? undefined
      : {
          ...(existing?.content ?? { caseSensitive: false, wholeWord: false }),
          query: params.contentQuery,
          regex: params.contentRegex,
        };

  return {
    schemaVersion: 1,
    scope:
      existing === undefined
        ? { locations: [root], recurse: params.recurse, showHidden }
        : { ...existing.scope, recurse: params.recurse },
    ...(name === undefined ? {} : { name }),
    entryKinds: params.entryKinds ?? existing?.entryKinds ?? ['file'],
    mimeTypes: params.mimeTypes ?? [],
    ...(params.minSizeBytes === undefined ? {} : { minSizeBytes: params.minSizeBytes }),
    ...(params.maxSizeBytes === undefined ? {} : { maxSizeBytes: params.maxSizeBytes }),
    ...(params.modifiedAfter === undefined ? {} : { modifiedAfter: params.modifiedAfter }),
    ...(params.modifiedBefore === undefined ? {} : { modifiedBefore: params.modifiedBefore }),
    ...(content === undefined ? {} : { content }),
    gitStatuses: params.gitStatuses ?? existing?.gitStatuses ?? [],
    tags: params.tags ?? [],
    metadata: params.metadata ?? existing?.metadata ?? {},
  };
}

/**
 * Factory function to create a FindFilesController.
 */
export function createFindFilesController(
  context: FindFilesControllerContext,
): FindFilesController {
  const refreshGenerationBySearchId = new Map<string, number>();

  function runSearch(
    query: SearchQuery,
    params: FindFilesSearchParams,
    target: SavedSearchOpenTarget,
    refresh?: {
      readonly paneId: PaneId;
      readonly searchId: string;
      readonly locationUri: string;
    },
    presentationLabel?: string,
  ): void {
    const workspace = context.getWorkspace();
    const root = query.scope.locations[0];
    if (workspace === undefined || root === undefined) return;
    const activePaneId =
      refresh?.paneId ?? context.getActiveDirectory()?.paneId ?? workspace.activePaneId;
    if (activePaneId === undefined) return;
    const paneId =
      target === 'otherPane'
        ? (workspace.paneOrder.find((candidate) => candidate !== activePaneId) ?? activePaneId)
        : activePaneId;

    const previousSearchId = refresh?.searchId ?? context.getFindFilesSearchId();
    if (previousSearchId !== undefined) {
      context.clearFindFilesTargetPane(previousSearchId);
      void context
        .getClient()
        .cancelSearch(previousSearchId)
        .catch(() => undefined);
    }

    const nextGeneration =
      refresh === undefined
        ? context.getFindFilesGeneration() + 1
        : (refreshGenerationBySearchId.get(refresh.searchId) ?? 0) + 1;
    if (refresh === undefined) {
      context.setFindFilesGeneration(nextGeneration);
      context.setFindFilesError(undefined);
      context.setFindFilesSearchId(undefined);
    } else {
      refreshGenerationBySearchId.set(refresh.searchId, nextGeneration);
    }

    context.setFindFilesSearchStartPending(true);
    void context
      .getClient()
      .startSearch({
        query: query.name?.pattern ?? '',
        roots: query.scope.locations,
        workspaceId: workspace.id,
        structuredQuery: query,
      })
      .then((result) => {
        const stale =
          refresh === undefined
            ? nextGeneration !== context.getFindFilesGeneration()
            : nextGeneration !== refreshGenerationBySearchId.get(refresh.searchId) ||
              context.getPaneLocationUri(refresh.paneId) !== refresh.locationUri;
        if (stale) {
          context.setFindFilesSearchStartPending(false);
          void context
            .getClient()
            .cancelSearch(result.searchId)
            .catch(() => undefined);
          return;
        }
        if (target !== 'newTab') context.setFindFilesTargetPane(result.searchId, paneId);
        if (refresh === undefined) context.setFindFilesSearchId(result.searchId);
        context.setFindFilesSearchStartPending(false);
        context.getFindFilesRootsByLocationUri().set(result.location.uri, root);
        const executionMode =
          context.getSearchExecutionMode?.(result.searchId) ?? result.executionMode;
        context
          .getFindFilesPresentationsByLocationUri()
          .set(result.location.uri, searchPresentation(params, executionMode, presentationLabel));
        context.getFindFilesParamsByLocationUri().set(result.location.uri, params);
        context.getFindFilesQueriesByLocationUri().set(result.location.uri, query);
        if (result.limitations.length > 0) {
          context.reportLimitations(
            result.limitations
              .map(
                ({ providerId, unevaluatedPredicates }) =>
                  `${providerId}: ${unevaluatedPredicates.join(', ')}`,
              )
              .join('; '),
          );
        }
        if (target === 'newTab') context.openTabAt(paneId, result.location, root);
        if (refresh === undefined) {
          context.setFindFilesOpen(false);
          context.setFindFilesRoot(undefined);
        }
        context.redraw();
      })
      .catch((error: unknown) => {
        const stale =
          refresh === undefined
            ? nextGeneration !== context.getFindFilesGeneration()
            : nextGeneration !== refreshGenerationBySearchId.get(refresh.searchId) ||
              context.getPaneLocationUri(refresh.paneId) !== refresh.locationUri;
        context.setFindFilesSearchStartPending(false);
        if (stale) return;
        if (refresh === undefined) {
          context.setFindFilesError(
            error instanceof Error ? error.message : t('search', 'unableToStart'),
          );
        }
        context.redraw();
      });
  }

  return {
    openFindFiles(): void {
      const active = context.getActiveDirectory();
      if (active === undefined) return;
      const root =
        context.getFindFilesRootsByLocationUri().get(active.location.uri) ?? active.location;
      context.setFindFilesRoot(root);
      context.setFindFilesOpen(true);
    },

    closeFindFiles(): void {
      const searchId = context.getFindFilesSearchId();
      if (searchId !== undefined) {
        void context
          .getClient()
          .cancelSearch(searchId)
          .catch(() => undefined);
      }
      context.setFindFilesGeneration(context.getFindFilesGeneration() + 1);
      context.setFindFilesOpen(false);
      context.setFindFilesRoot(undefined);
      context.setFindFilesSearchId(undefined);
      context.setFindFilesError(undefined);
    },

    dismissFindFiles(): void {
      context.setFindFilesOpen(false);
      context.setFindFilesRoot(undefined);
      context.setFindFilesError(undefined);
    },

    startFindFilesSearch(params: FindFilesSearchParams): void {
      const root = context.getFindFilesRoot();
      const workspace = context.getWorkspace();
      if (root === undefined || workspace === undefined) return;
      const searchPaneId = context.getActiveDirectory()?.paneId ?? workspace.activePaneId;
      runSearch(
        searchQueryFromParams(
          root,
          params,
          searchPaneId === undefined ? false : this.activeShowHidden(searchPaneId),
        ),
        params,
        'currentPane',
      );
    },

    startSavedSearch(saved: SavedSearch, target: SavedSearchOpenTarget): void {
      const name = saved.query.name?.pattern ?? '';
      runSearch(
        saved.query,
        {
          filenameQuery: name,
          contentRegex: saved.query.content?.regex ?? false,
          recurse: saved.query.scope.recurse,
          entryKinds: saved.query.entryKinds,
          mimeTypes: saved.query.mimeTypes,
          gitStatuses: saved.query.gitStatuses,
          tags: saved.query.tags,
          metadata: saved.query.metadata,
          ...(saved.query.content === undefined ? {} : { contentQuery: saved.query.content.query }),
          ...(saved.query.minSizeBytes === undefined
            ? {}
            : { minSizeBytes: saved.query.minSizeBytes }),
          ...(saved.query.maxSizeBytes === undefined
            ? {}
            : { maxSizeBytes: saved.query.maxSizeBytes }),
          ...(saved.query.modifiedAfter === undefined
            ? {}
            : { modifiedAfter: saved.query.modifiedAfter }),
          ...(saved.query.modifiedBefore === undefined
            ? {}
            : { modifiedBefore: saved.query.modifiedBefore }),
        },
        target,
        undefined,
        saved.name,
      );
    },

    refreshSearch(searchLocationUri: string, paneId: PaneId): void {
      const query = context.getFindFilesQueriesByLocationUri().get(searchLocationUri);
      const params = context.getFindFilesParamsByLocationUri().get(searchLocationUri);
      if (query === undefined || params === undefined) return;
      const searchId = searchLocationUri.startsWith('search://local/')
        ? searchLocationUri.slice('search://local/'.length)
        : undefined;
      if (searchId === undefined || searchId.length === 0) return;
      runSearch(
        query,
        params,
        'currentPane',
        { paneId, searchId, locationUri: searchLocationUri },
        context.getFindFilesPresentationsByLocationUri().get(searchLocationUri)?.label,
      );
    },

    activeShowHidden(paneId: PaneId): boolean {
      const workspace = context.getWorkspace();
      const pane = workspace?.panesById[paneId];
      const tab = pane === undefined ? undefined : pane.tabsById[pane.activeTabId];
      return tab?.view.showHidden ?? false;
    },
  };
}
