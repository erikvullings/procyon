import m from 'mithril';
import { toast } from 'mithril-materialized';
import type { FileManagerClient } from '../api/client/file-manager-client';
import type { BasketState } from '../features/basket/basket';
import {
  addToBasket,
  basketItemFromEntry,
  basketSummary,
  classifyBasketAbsence,
  emptyBasket,
  loadBasket,
  refreshBasket,
  saveBasket,
  withBasketFolderSize,
} from '../features/basket/basket';
import { parentLocation } from '../features/navigation/navigation';
import { isParentEntry } from '../features/panes/parent-entry';
import {
  emptySelection,
  getSelectedEntriesOrCursor,
  reduceSelection,
} from '../features/selection/selection';
import { dispatchWorkspaceCommand } from '../features/workspace/dispatch-workspace-command';
import { t } from '../i18n';
import type {
  EntrySummary,
  Location,
  PaneId,
  SortDescriptor,
  TabId,
  TabProjection,
  WorkspaceProjection,
} from '../models';
import { applyAppPatches, basketPatch } from '../state';
import { effectiveSort, tabKey, workspaceErrorMessage } from './shell-helpers';
import type { ShellState } from './shell-state';

/** Sets up the basketWiring feature wiring and returns its functions. */
/** The narrow cross-cutting surface basket wiring needs from the shell. */
export interface BasketCore {
  readonly activeTabKey: (paneId: PaneId) => string;
  readonly entriesSortedFor: (
    key: string,
    entries: readonly EntrySummary[],
    sort: readonly SortDescriptor[],
    foldersFirst: boolean,
    groupByParentPath?: boolean,
  ) => readonly EntrySummary[];
  readonly entriesFilteredFor: (
    key: string,
    entries: readonly EntrySummary[],
    query: string,
  ) => readonly EntrySummary[];
  readonly quickFilterQueryFor: (key: string, tab: TabProjection | undefined) => string;
  readonly selectTab: (client: FileManagerClient, paneId: PaneId, tabId: TabId) => void;
  readonly activatePane: (client: FileManagerClient, paneId: PaneId) => Promise<void>;
  readonly replaceWorkspace: (next: WorkspaceProjection) => void;
  readonly refetchAffectedPanes: (
    paneId?: PaneId,
    options?: {
      readonly background?: boolean;
      readonly settleMs?: number;
      readonly force?: boolean;
    },
  ) => void;
}

export function createBasketWiring(state: ShellState, core: BasketCore) {
  function basketFor(workspaceId: string): BasketState {
    if (state.appState === undefined) return emptyBasket;
    const cached = state.appState.baskets[workspaceId];
    if (cached !== undefined) return cached;
    let restored = emptyBasket;
    try {
      restored = loadBasket(localStorage, workspaceId);
    } catch (error) {
      toast({ html: workspaceErrorMessage(error, t('basket', 'restoreFailed')) });
    }
    state.appState = applyAppPatches(state.appState, basketPatch(workspaceId, restored));
    return restored;
  }

  function updateBasket(workspaceId: string, next: BasketState): void {
    if (state.appState === undefined) return;
    try {
      saveBasket(localStorage, workspaceId, next);
    } catch (error) {
      toast({ html: workspaceErrorMessage(error, t('basket', 'saveFailed')) });
      return;
    }
    state.appState = applyAppPatches(state.appState, basketPatch(workspaceId, next));
    m.redraw();
  }

  function basketVirtualLocation(workspaceId: string): Location {
    return { providerId: 'basket', uri: `basket://local/${encodeURIComponent(workspaceId)}` };
  }

  function basketEntries(state: BasketState): readonly EntrySummary[] {
    return state.items.map((item) => ({
      id: item.id,
      location: item.location,
      name: item.name,
      kind: item.kind,
      hidden: false,
      readOnly: item.status !== 'ready',
      metadataRevision: item.metadataRevision,
      ...(item.kind === 'directory'
        ? item.folderSize === undefined
          ? {}
          : { size: item.folderSize }
        : item.size === undefined
          ? {}
          : { size: item.size }),
      ...(item.modifiedAt === undefined ? {} : { modifiedAt: item.modifiedAt }),
    }));
  }

  function syncBasketVirtualDirectory(paneId: PaneId, tabId: TabId, workspaceId: string): void {
    const key = tabKey(paneId, tabId);
    const basket = basketFor(workspaceId);
    const entries = basketEntries(basket);
    const summary = basketSummary(basket);
    state.directories.set(key, {
      state: { type: 'loaded' },
      entries,
      location: basketVirtualLocation(workspaceId),
      writable: false,
      hasMore: false,
      totalKnownEntries: entries.length,
      totalKnownSize: summary.knownSize,
      totalKnownFileCount: summary.fileCount,
    });
    const selection = state.selections.get(key) ?? emptySelection;
    if (entries.length === 0) {
      state.selections.set(key, emptySelection);
    } else if (!entries.some((entry) => entry.id === selection.cursorEntryId)) {
      // A transient tab may still have its borrowed folder's cursor, or have opened empty.
      const tab = state.workspace?.panesById[paneId]?.tabsById[tabId];
      const sorted = core.entriesSortedFor(
        key,
        entries,
        effectiveSort(tab?.view.sort ?? []),
        tab?.view.foldersFirst ?? false,
        true,
      );
      const first = core.entriesFilteredFor(key, sorted, core.quickFilterQueryFor(key, tab))[0];
      if (first !== undefined) {
        state.selections.set(
          key,
          reduceSelection(
            selection,
            { type: 'positionCursor', entryId: first.id },
            entries.map((entry) => entry.id),
          ),
        );
      }
    }
  }

  function selectedBasketKeys(paneId: PaneId): readonly string[] {
    const key = core.activeTabKey(paneId);
    const directory = state.directories.get(key);
    const basket = state.workspace === undefined ? emptyBasket : basketFor(state.workspace.id);
    const selection = state.selections.get(key);
    const selected = getSelectedEntriesOrCursor(selection, directory?.entries ?? []);
    if (selected.length === 0) return [];
    const selectedRefs = new Set(
      selected.map((entry) => `${entry.location.providerId}\0${entry.id}`),
    );
    return basket.items
      .filter((item) => selectedRefs.has(`${item.location.providerId}\0${item.id}`))
      .map((item) => item.key);
  }

  function removeSelectedBasketItems(paneId: PaneId): boolean {
    const current = state.workspace;
    if (
      current === undefined ||
      !state.basketTabIds.has(current.panesById[paneId]?.activeTabId ?? '')
    ) {
      return false;
    }
    const keys = new Set(selectedBasketKeys(paneId));
    if (keys.size === 0) return true;
    const before = basketFor(current.id);
    const removedIds = new Set(
      before.items.filter((item) => keys.has(item.key)).map((item) => item.id),
    );
    updateBasket(current.id, {
      ...before,
      items: before.items.filter((item) => !keys.has(item.key)),
      selectedKeys: before.selectedKeys.filter((key) => !keys.has(key)),
    });
    const tabId = current.panesById[paneId]?.activeTabId;
    if (tabId !== undefined) {
      const key = tabKey(paneId, tabId);
      const directory = state.directories.get(key);
      const tab = current.panesById[paneId]?.tabsById[tabId];
      const sorted = core.entriesSortedFor(
        key,
        directory?.entries ?? [],
        effectiveSort(tab?.view.sort ?? []),
        tab?.view.foldersFirst ?? false,
        true,
      );
      const visible = core.entriesFilteredFor(key, sorted, core.quickFilterQueryFor(key, tab));
      state.selections.set(
        key,
        reduceSelection(
          state.selections.get(key) ?? emptySelection,
          { type: 'prune', removedEntryIds: [...removedIds] },
          visible.map((entry) => entry.id),
        ),
      );
    }
    return true;
  }

  async function prepareFileOperationEntries(
    paneId: PaneId,
    entries: readonly EntrySummary[],
  ): Promise<readonly EntrySummary[] | undefined> {
    const current = state.workspace;
    if (
      current === undefined ||
      !state.basketTabIds.has(current.panesById[paneId]?.activeTabId ?? '')
    ) {
      return entries;
    }
    await recheckBasket(current.id, true);
    const tabId = current.panesById[paneId]?.activeTabId;
    if (tabId !== undefined) syncBasketVirtualDirectory(paneId, tabId, current.id);
    const key = core.activeTabKey(paneId);
    const refreshed = getSelectedEntriesOrCursor(
      state.selections.get(key),
      state.directories.get(key)?.entries ?? [],
    );
    const unavailable = refreshed.filter((entry) => entry.readOnly);
    if (unavailable.length > 0) {
      toast({ html: t('basket', 'unavailableCount', unavailable.length) });
      return undefined;
    }
    if (refreshed.length === 0) {
      toast({ html: t('basket', 'noAvailable') });
      return undefined;
    }
    return refreshed;
  }

  function basketVisible(): boolean {
    return (
      state.workspace?.paneOrder.some((paneId) =>
        state.basketTabIds.has(state.workspace?.panesById[paneId]?.activeTabId ?? ''),
      ) ?? false
    );
  }

  function scheduleBasketRefresh(): void {
    const workspaceId = state.workspace?.id;
    if (workspaceId === undefined || !basketVisible()) return;
    if (state.basketRefreshTimer !== undefined) clearTimeout(state.basketRefreshTimer);
    state.basketRefreshTimer = setTimeout(() => {
      state.basketRefreshTimer = undefined;
      void recheckBasket(workspaceId)
        .then(() => {
          if (state.workspace?.id === workspaceId && basketVisible()) {
            void calculateBasketFolderSizes(workspaceId, true).catch(() => undefined);
          }
        })
        .catch((error: unknown) => {
          toast({ html: workspaceErrorMessage(error, t('basket', 'checkFailed')) });
        });
    }, 150);
  }

  function basketSourcePaneId(): PaneId | undefined {
    if (state.workspace === undefined) return undefined;
    const active = state.workspace.activePaneId;
    if (!state.basketTabIds.has(state.workspace.panesById[active]?.activeTabId ?? ''))
      return active;
    return state.workspace.paneOrder.find(
      (paneId) => !state.basketTabIds.has(state.workspace?.panesById[paneId]?.activeTabId ?? ''),
    );
  }

  function collectSelection(): void {
    const current = state.workspace;
    const paneId = basketSourcePaneId();
    if (current === undefined || paneId === undefined) return;
    const tabId = current.panesById[paneId]?.activeTabId;
    if (tabId === undefined || state.basketTabIds.has(tabId)) return;
    const key = core.activeTabKey(paneId);
    const selected = getSelectedEntriesOrCursor(
      state.selections.get(key),
      state.directories.get(key)?.entries ?? [],
    ).filter((entry) => !isParentEntry(entry.id));
    if (selected.length === 0) return;
    try {
      updateBasket(current.id, addToBasket(basketFor(current.id), selected));
      if (basketVisible()) {
        void calculateBasketFolderSizes(current.id).catch((error: unknown) => {
          toast({
            html: workspaceErrorMessage(error, t('basket', 'folderSizeFailed', 1)),
          });
        });
      }
    } catch (error) {
      toast({ html: workspaceErrorMessage(error, t('basket', 'collectFailed')) });
    }
  }

  function basketTabLocation(): { paneId: PaneId; tabId: TabId } | undefined {
    const current = state.workspace;
    if (current === undefined) return undefined;
    for (const paneId of current.paneOrder) {
      const tabId = current.panesById[paneId]?.tabOrder.find((id) => state.basketTabIds.has(id));
      if (tabId !== undefined) return { paneId, tabId };
    }
    return undefined;
  }

  function toggleBasket(): void {
    const open = basketTabLocation();
    if (open === undefined) {
      openBasket();
      return;
    }
    state.tabController.requestCloseTab(open.paneId, open.tabId);
  }

  function openBasket(): void {
    const current = state.workspace;
    if (current === undefined) return;
    for (const paneId of current.paneOrder) {
      const tabId = current.panesById[paneId]?.tabOrder.find((id) => state.basketTabIds.has(id));
      if (tabId !== undefined) {
        core.selectTab(state.client, paneId, tabId);
        inspectBasketOnOpen(current.id);
        return;
      }
    }
    const paneId = current.activePaneId;
    const location =
      current.panesById[paneId]?.tabsById[current.panesById[paneId]?.activeTabId ?? '']?.location;
    if (location === undefined) return;
    void dispatchWorkspaceCommand(
      state.client,
      {
        type: 'addTransientTab',
        workspaceId: current.id,
        paneId,
        location,
        expectedRevision: current.revision,
      },
      (next) => {
        state.workspace = next;
        const tabId = next.panesById[paneId]?.activeTabId;
        if (tabId !== undefined) state.basketTabIds.add(tabId);
        basketFor(next.id);
        inspectBasketOnOpen(next.id);
        m.redraw();
      },
    ).catch((error: unknown) => {
      toast({ html: workspaceErrorMessage(error, t('basket', 'openFailed')) });
    });
  }

  async function recheckBasket(workspaceId: string, selectedOnly = false): Promise<BasketState> {
    const before = basketFor(workspaceId);
    const visibleEntries = [...state.directories.values()].flatMap(
      (directory) => directory.entries,
    );
    const selected =
      selectedOnly && state.workspace?.id === workspaceId
        ? new Set(
            state.workspace.paneOrder
              .filter((paneId) =>
                state.basketTabIds.has(state.workspace?.panesById[paneId]?.activeTabId ?? ''),
              )
              .flatMap((paneId) => selectedBasketKeys(paneId)),
          )
        : undefined;
    const parentCache = new Map<string, Promise<readonly EntrySummary[]>>();
    const entriesInParent = (location: Location): Promise<readonly EntrySummary[]> => {
      const parent = parentLocation(location);
      const cacheKey = `${parent.providerId}\0${parent.uri}`;
      const cached = parentCache.get(cacheKey);
      if (cached !== undefined) return cached;
      const request = (async (): Promise<readonly EntrySummary[]> => {
        const entries: EntrySummary[] = [];
        // Listing sessions are UUID-keyed and cancel older requests sharing their pane ID.
        const paneId = crypto.randomUUID();
        let continuationToken: string | undefined;
        do {
          const snapshot = await state.client.listDirectory({
            workspaceId,
            paneId,
            requestId: crypto.randomUUID(),
            location: parent,
            ...(continuationToken === undefined ? {} : { continuationToken }),
            showHidden: true,
          });
          entries.push(...snapshot.entries);
          continuationToken = snapshot.continuationToken;
          if (!snapshot.hasMore) break;
        } while (continuationToken !== undefined);
        return entries;
      })();
      parentCache.set(cacheKey, request);
      return request;
    };
    m.redraw();
    try {
      const checked = await refreshBasket(
        before,
        async (item) => {
          try {
            await state.client.getEntryMetadata({
              entryId: item.id,
              location: item.location,
              verifyIdentity: true,
            });
            const fresh = (await entriesInParent(item.location)).find(
              (entry) =>
                entry.id === item.id &&
                entry.location.providerId === item.location.providerId &&
                entry.location.uri === item.location.uri,
            );
            return fresh === undefined
              ? { ...item, status: 'ready' }
              : basketItemFromEntry(fresh, item);
          } catch (error) {
            if (typeof error === 'object' && error !== null && 'code' in error) {
              if (error.code === 'notFound') {
                return classifyBasketAbsence(item, visibleEntries);
              }
              if (error.code === 'invalidRequest') return 'stale';
            }
            throw error;
          }
        },
        selected,
      );
      if (basketFor(workspaceId) === before) updateBasket(workspaceId, checked);
      return checked;
    } finally {
      m.redraw();
    }
  }

  async function calculateBasketFolderSizes(
    workspaceId: string,
    refreshKnownSizes = false,
  ): Promise<void> {
    state.basketFolderScan?.abort();
    const controller = new AbortController();
    state.basketFolderScan = controller;
    const current = basketFor(workspaceId);
    const folders = current.items.filter(
      (item) =>
        item.kind === 'directory' &&
        item.status === 'ready' &&
        (refreshKnownSizes || item.folderSize === undefined),
    );
    if (refreshKnownSizes && folders.some((item) => item.folderSize !== undefined)) {
      const keys = new Set(folders.map((item) => item.key));
      updateBasket(workspaceId, {
        ...current,
        items: current.items.map((item) => {
          if (!keys.has(item.key)) return item;
          const { folderSize: _previousSize, ...unmeasuredItem } = item;
          return unmeasuredItem;
        }),
      });
    }
    m.redraw();
    let index = 0;
    let failures = 0;
    try {
      await Promise.all(
        Array.from({ length: Math.min(2, folders.length) }, async () => {
          while (!controller.signal.aborted && index < folders.length) {
            const item = folders[index++];
            if (item === undefined) continue;
            let result: Awaited<ReturnType<typeof state.client.calculateFolderSize>>;
            try {
              result = await state.client.calculateFolderSize(
                { location: item.location },
                controller.signal,
              );
            } catch {
              if (controller.signal.aborted) return;
              failures += 1;
              continue;
            }
            if (controller.signal.aborted) return;
            const current = basketFor(workspaceId);
            if (
              current.items.some(
                (candidate) => candidate.key === item.key && candidate.status === 'ready',
              )
            ) {
              updateBasket(workspaceId, withBasketFolderSize(current, item.key, result.totalBytes));
            }
          }
        }),
      );
      if (failures > 0 && !controller.signal.aborted) {
        toast({ html: t('basket', 'folderSizeFailed', failures) });
      }
    } finally {
      if (state.basketFolderScan === controller) {
        state.basketFolderScan = undefined;
        m.redraw();
      }
    }
  }

  function inspectBasketOnOpen(workspaceId: string): void {
    void recheckBasket(workspaceId)
      .then(() => {
        if (state.workspace?.id === workspaceId && basketVisible()) {
          void calculateBasketFolderSizes(workspaceId, true).catch((error: unknown) => {
            toast({
              html: workspaceErrorMessage(error, t('basket', 'folderSizeFailed', 1)),
            });
          });
        }
      })
      .catch((error: unknown) => {
        toast({ html: workspaceErrorMessage(error, t('basket', 'checkFailed')) });
      });
  }

  return {
    basketFor,
    updateBasket,
    basketVirtualLocation,
    basketEntries,
    syncBasketVirtualDirectory,
    selectedBasketKeys,
    removeSelectedBasketItems,
    prepareFileOperationEntries,
    basketVisible,
    scheduleBasketRefresh,
    basketSourcePaneId,
    collectSelection,
    basketTabLocation,
    toggleBasket,
    openBasket,
    recheckBasket,
    calculateBasketFolderSizes,
    inspectBasketOnOpen,
  };
}
