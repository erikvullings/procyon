import m from 'mithril';
import { toast } from 'mithril-materialized';
import { mergeCleanupCandidates } from '../features/disk-usage/cleanup-candidates';
import {
  dismissOperation,
  mergeOperationHistory,
  shouldAutoDismissOperation,
} from '../features/operations/operation-state';
import { reduceSelection } from '../features/selection/selection';
import { dispatchWorkspaceCommand } from '../features/workspace/dispatch-workspace-command';
import { t } from '../i18n';
import type {
  InterruptedTransfer,
  Location,
  Operation,
  OperationId,
  PaneId,
  ScanDiskUsageResult,
  TabId,
  WorkspaceProjection,
} from '../models';
import {
  persistDismissedOperationIds,
  removeDiskUsageNodes,
  tabKey,
  workspaceErrorMessage,
} from './shell-helpers';
import type { DiskUsageTabEntry, ShellState } from './shell-state';

/** The narrow cross-cutting surface {@link createOperationsWiring} needs from the shell. */
export interface OperationsCore {
  readonly activeDirectory: () =>
    | { readonly paneId: PaneId; readonly location: Location }
    | undefined;
  readonly replaceWorkspace: (next: WorkspaceProjection) => void;
}

/** Sets up the opsWiring feature wiring and returns its functions. */
export function createOperationsWiring(state: ShellState, core: OperationsCore) {
  function rememberDismissedOperation(operationId: OperationId): void {
    state.dismissedOperationIds.add(operationId);
    persistDismissedOperationIds(state.dismissedOperationIds);
  }

  function clearDismissedOperation(operationId: OperationId): void {
    if (!state.dismissedOperationIds.delete(operationId)) return;
    persistDismissedOperationIds(state.dismissedOperationIds);
  }

  function scheduleAutoDismiss(operationId: OperationId, delayMs: number): void {
    const existing = state.autoDismissTimers.get(operationId);
    if (existing !== undefined) clearTimeout(existing);
    state.autoDismissTimers.set(
      operationId,
      setTimeout(() => {
        state.autoDismissTimers.delete(operationId);
        const current = state.operations.byId[operationId];
        if (current !== undefined && shouldAutoDismissOperation(current)) {
          state.operations = dismissOperation(state.operations, operationId);
          m.redraw();
        }
      }, delayMs),
    );
  }

  function cancelAutoDismiss(operationId: OperationId): void {
    const existing = state.autoDismissTimers.get(operationId);
    if (existing === undefined) return;
    clearTimeout(existing);
    state.autoDismissTimers.delete(operationId);
  }

  function clearOperationSourceSelections(operation: Operation): void {
    const sourceUris = new Set(operation.sources.map((source) => source.location.uri));
    for (const [key, selection] of state.selections) {
      const directory = state.directories.get(key);
      if (directory === undefined || selection.selectedEntryIds.length === 0) continue;
      const sourceIds = new Set(
        directory.entries
          .filter((entry) => sourceUris.has(entry.location.uri))
          .map((entry) => entry.id),
      );
      const selectedEntryIds = selection.selectedEntryIds.filter((id) => !sourceIds.has(id));
      if (selectedEntryIds.length !== selection.selectedEntryIds.length) {
        state.selections.set(key, { ...selection, selectedEntryIds });
      }
    }
  }

  function diskUsageRootName(location: Location): string {
    const segment = location.uri.replace(/\/+$/u, '').split('/').at(-1);
    if (segment === undefined || segment === '') return '/';
    try {
      return decodeURIComponent(segment);
    } catch {
      return segment;
    }
  }

  function replaceDiskUsageNode(
    node: ScanDiskUsageResult['root'],
    replacement: ScanDiskUsageResult['root'],
  ): ScanDiskUsageResult['root'] {
    if (node.location.uri === replacement.location.uri) return replacement;
    let changed = false;
    const children = node.children.map((child) => {
      const next = replaceDiskUsageNode(child, replacement);
      changed ||= next !== child;
      return next;
    });
    if (!changed) return node;
    const previousLogicalBytes = node.children.reduce(
      (total, child) => total + child.logicalBytes,
      0,
    );
    const previousPhysicalBytes = node.children.reduce(
      (total, child) => total + child.physicalBytes,
      0,
    );
    const nextLogicalBytes = children.reduce((total, child) => total + child.logicalBytes, 0);
    const nextPhysicalBytes = children.reduce((total, child) => total + child.physicalBytes, 0);
    return {
      ...node,
      logicalBytes: Math.max(0, node.logicalBytes - previousLogicalBytes + nextLogicalBytes),
      physicalBytes: Math.max(0, node.physicalBytes - previousPhysicalBytes + nextPhysicalBytes),
      children,
    };
  }

  function applyDiskUsageProgress(
    scanId: string,
    result: ScanDiskUsageResult,
    isComplete: boolean,
  ): void {
    const entry = [...state.diskUsageByTab.values()].find(
      (candidate) => candidate.scanId === scanId,
    );
    if (entry === undefined || entry.progressComplete) return;
    const currentResult = entry.state.type === 'loaded' ? entry.state.result : undefined;
    const baseResult = entry.expansionBaseResult ?? currentResult;
    const nextResult =
      entry.expansionLocation === undefined || baseResult === undefined
        ? result
        : {
            root: replaceDiskUsageNode(baseResult.root, result.root),
            unreadableEntries: baseResult.unreadableEntries,
            unreadable: baseResult.unreadable ?? [],
            scannedEntries: result.scannedEntries ?? 0,
            cleanupCandidates: mergeCleanupCandidates(
              baseResult.cleanupCandidates ?? [],
              result.cleanupCandidates ?? [],
              entry.expansionLocation,
            ),
          };
    entry.state = {
      type: 'loaded',
      result: nextResult,
      scanning: !isComplete,
      finalizing: false,
    };
    if (isComplete) {
      entry.progressComplete = true;
      entry.expansionLocation = undefined;
      entry.expansionBaseResult = undefined;
    }
  }

  function applyDiskUsageFailure(scanId: string, message: string): void {
    const entry = [...state.diskUsageByTab.values()].find(
      (candidate) => candidate.scanId === scanId,
    );
    if (entry === undefined || entry.progressComplete) return;
    entry.progressComplete = true;
    entry.state =
      entry.state.type === 'loaded'
        ? { ...entry.state, scanning: false, error: message }
        : { type: 'error', message };
  }

  function applyDiskUsageFinalizing(scanId: string, scannedEntries: number): void {
    const entry = [...state.diskUsageByTab.values()].find(
      (candidate) => candidate.scanId === scanId,
    );
    if (entry === undefined || entry.progressComplete || entry.state.type !== 'loaded') return;
    entry.state = {
      ...entry.state,
      scanning: true,
      finalizing: true,
      result: { ...entry.state.result, scannedEntries },
    };
  }

  function startDiskUsageScan(
    paneId: PaneId,
    tabId: TabId,
    location: Location,
    expansionLocation?: Location,
  ): void {
    const key = tabKey(paneId, tabId);
    const existing = state.diskUsageByTab.get(key);
    existing?.abort.abort();
    const currentWorkspace = state.workspace;
    if (currentWorkspace === undefined) return;
    const abort = new AbortController();
    const scanId = crypto.randomUUID();
    const entry: DiskUsageTabEntry =
      existing === undefined
        ? {
            key,
            paneId,
            tabId,
            location,
            abort,
            scanId,
            progressComplete: false,
            expansionLocation,
            expansionBaseResult: undefined,
            state: {
              type: 'loading',
              rootName: diskUsageRootName(location),
            },
          }
        : {
            ...existing,
            location: expansionLocation === undefined ? location : existing.location,
            abort,
            scanId,
            progressComplete: false,
            expansionLocation,
            expansionBaseResult:
              expansionLocation !== undefined && existing.state.type === 'loaded'
                ? existing.state.result
                : undefined,
            state:
              expansionLocation === undefined
                ? { type: 'loading', rootName: diskUsageRootName(location) }
                : existing.state.type === 'loaded'
                  ? { ...existing.state, scanning: true }
                  : existing.state,
          };
    state.diskUsageByTab.set(key, entry);
    m.redraw();
    void state.client
      .scanDiskUsage(
        {
          workspaceId: currentWorkspace.id,
          scanId,
          location,
          expandRoot: expansionLocation !== undefined,
        },
        abort.signal,
      )
      .then(() => undefined)
      .catch((error: unknown) => {
        const current = state.diskUsageByTab.get(entry.key);
        if (abort.signal.aborted || current?.scanId !== scanId || current.progressComplete) return;
        const message = workspaceErrorMessage(error, t('diskUsage', 'scanFailed'));
        current.progressComplete = true;
        current.state =
          current.state.type === 'loaded'
            ? { ...current.state, scanning: false, error: message }
            : { type: 'error', message };
        m.redraw();
      });
  }

  function expandDiskUsageFolder(key: string, location: Location): void {
    const entry = state.diskUsageByTab.get(key);
    if (entry === undefined) return;
    startDiskUsageScan(entry.paneId, entry.tabId, location, location);
  }

  function scanDiskUsageFolder(key: string, location: Location): void {
    const entry = state.diskUsageByTab.get(key);
    if (entry === undefined) return;
    startDiskUsageScan(entry.paneId, entry.tabId, location);
  }

  function stopDiskUsage(key: string): void {
    const entry = state.diskUsageByTab.get(key);
    if (entry === undefined || entry.progressComplete) return;
    entry.abort.abort();
    entry.progressComplete = true;
    if (entry.state.type === 'loaded') {
      entry.state = { ...entry.state, scanning: false };
    } else {
      entry.state = { type: 'cancelled', rootName: diskUsageRootName(entry.location) };
    }
    void state.client.cancelDiskUsage(entry.scanId).catch((error: unknown) => {
      console.warn('Failed to cancel disk-usage scan', error);
    });
    m.redraw();
  }

  function openDiskUsage(): void {
    const active = core.activeDirectory();
    const currentWorkspace = state.workspace;
    const targetPaneId = currentWorkspace?.paneOrder.find(
      (candidate) => candidate !== active?.paneId,
    );
    if (
      active === undefined ||
      currentWorkspace === undefined ||
      targetPaneId === undefined ||
      currentWorkspace.panesById[targetPaneId] === undefined
    )
      return;
    void dispatchWorkspaceCommand(
      state.client,
      {
        type: 'addTransientTab',
        workspaceId: currentWorkspace.id,
        paneId: targetPaneId,
        location: active.location,
        expectedRevision: currentWorkspace.revision,
      },
      core.replaceWorkspace,
    )
      .then((next) => {
        const tabId = next.panesById[targetPaneId]?.activeTabId;
        if (tabId !== undefined) {
          startDiskUsageScan(targetPaneId, tabId, active.location);
          state.focusPane?.(targetPaneId);
        }
      })
      .catch((error: unknown) => {
        toast({ html: workspaceErrorMessage(error, t('diskUsage', 'scanFailed')) });
      });
  }

  function openDiskUsageFolder(
    paneId: PaneId,
    location: Location,
    preferredCursorName?: string,
  ): void {
    const oppositePaneId = state.workspace?.paneOrder.find((candidate) => candidate !== paneId);
    if (oppositePaneId !== undefined)
      void state.navigation.navigate(oppositePaneId, location, preferredCursorName);
  }

  function removeOperationSourcesFromSearchResults(operation: Operation): void {
    if (state.workspace === undefined || operation.sources.length === 0) return;
    const sourceUris = new Set(operation.sources.map((source) => source.location.uri));
    for (const pane of Object.values(state.workspace.panesById)) {
      for (const tabId of pane.tabOrder) {
        const tab = pane.tabsById[tabId];
        if (tab === undefined || !tab.location.uri.startsWith('search://')) continue;
        const key = tabKey(pane.id, tabId);
        const current = state.directories.get(key);
        if (current === undefined) continue;
        const removed = current.entries.filter((entry) => sourceUris.has(entry.location.uri));
        if (removed.length === 0) continue;
        state.directories.set(key, {
          ...current,
          entries: current.entries.filter((entry) => !sourceUris.has(entry.location.uri)),
        });
        const selection = state.selections.get(key);
        if (selection !== undefined) {
          state.selections.set(
            key,
            reduceSelection(
              selection,
              { type: 'prune', removedEntryIds: removed.map((entry) => entry.id) },
              current.entries.map((entry) => entry.id),
            ),
          );
        }
      }
    }
  }

  function removeOperationSourcesFromDiskUsage(operation: Operation): void {
    if (operation.sources.length === 0) return;
    const sourceUris = new Set(operation.sources.map((source) => source.location.uri));
    for (const entry of state.diskUsageByTab.values()) {
      if (entry.state.type !== 'loaded') continue;
      const root = removeDiskUsageNodes(entry.state.result.root, sourceUris);
      if (root === entry.state.result.root) continue;
      if (operation.state !== 'completed') {
        // Some sources may remain, so their sizes are unknown: scan the tab's root again.
        startDiskUsageScan(entry.paneId, entry.tabId, entry.location);
        continue;
      }
      if (!entry.progressComplete) {
        entry.abort.abort();
        entry.progressComplete = true;
        void state.client.cancelDiskUsage(entry.scanId).catch((error: unknown) => {
          console.warn('Failed to cancel stale disk-usage scan', error);
        });
      }
      entry.state =
        root === undefined
          ? { type: 'error', message: t('diskUsage', 'rootRemoved') }
          : {
              ...entry.state,
              scanning: false,
              result: { ...entry.state.result, root },
            };
    }
  }

  function setOperationCentreVisible(visible: boolean): void {
    const previous = state.workspace;
    if (previous === undefined || previous.operationCentre.visible === visible) return;
    const preferences = {
      ...previous.operationCentre,
      visible,
    };
    core.replaceWorkspace({ ...previous, operationCentre: preferences });
    void dispatchWorkspaceCommand(
      state.client,
      {
        type: 'updateOperationCentre',
        workspaceId: previous.id,
        preferences,
        expectedRevision: previous.revision,
      },
      core.replaceWorkspace,
    ).catch((error: unknown) => {
      if (state.workspace?.revision === previous.revision) core.replaceWorkspace(previous);
      toast({
        html: workspaceErrorMessage(error, t('shell', 'operationCentreUpdateFailed')),
      });
    });
  }

  /**
   * Loads what the previous session left interrupted (task 0300). The startup load tells the user
   * once; a reload after a failed action only refreshes the list, so a stale row (another window
   * already handled it) disappears instead of lingering behind an error.
   */
  function loadInterruptedTransfers(announce = true): void {
    void state.client
      .listInterruptedTransfers()
      .then(({ transfers }) => {
        if (state.removed) return;
        state.interruptedTransfers = transfers;
        if (announce && transfers.length > 0) {
          toast({ html: t('operation', 'interruptedToast') });
        }
        m.redraw();
      })
      .catch(() => undefined);
  }

  function withoutInterruptedTransfer(operationId: OperationId): readonly InterruptedTransfer[] {
    return state.interruptedTransfers.filter((transfer) => transfer.operationId !== operationId);
  }

  async function settleInterruptedTransfer(
    operationId: OperationId,
    act: () => Promise<unknown>,
  ): Promise<void> {
    state.interruptedTransferBusyId = operationId;
    state.interruptedTransferError = undefined;
    try {
      await act();
      state.interruptedTransfers = withoutInterruptedTransfer(operationId);
    } catch {
      state.interruptedTransferError = t('operation', 'interruptedActionFailed');
      loadInterruptedTransfers(false);
    } finally {
      state.interruptedTransferBusyId = undefined;
      m.redraw();
    }
  }

  function restartInterruptedTransfer(operationId: OperationId): void {
    void settleInterruptedTransfer(operationId, async () => {
      const restarted = await state.client.restartInterruptedTransfer(operationId);
      state.operations = mergeOperationHistory(state.operations, [restarted]);
    });
  }

  function discardInterruptedTransfer(operationId: OperationId): void {
    void settleInterruptedTransfer(operationId, () =>
      state.client.discardInterruptedTransfer(operationId),
    );
  }

  function toggleOperationCentre(): void {
    setOperationCentreVisible(state.workspace?.operationCentre.visible !== true);
  }

  function showAllOperations(): void {
    void state.client
      .listOperations()
      .then((listed) => {
        state.dismissedOperationIds.clear();
        persistDismissedOperationIds(state.dismissedOperationIds);
        state.operations = mergeOperationHistory(state.operations, listed);
        m.redraw();
      })
      .catch((error: unknown) => {
        toast({
          html: workspaceErrorMessage(error, t('operation', 'historyLoadFailed')),
        });
      });
  }

  return {
    loadInterruptedTransfers,
    restartInterruptedTransfer,
    discardInterruptedTransfer,
    rememberDismissedOperation,
    clearDismissedOperation,
    scheduleAutoDismiss,
    cancelAutoDismiss,
    clearOperationSourceSelections,
    diskUsageRootName,
    replaceDiskUsageNode,
    applyDiskUsageProgress,
    applyDiskUsageFailure,
    applyDiskUsageFinalizing,
    startDiskUsageScan,
    expandDiskUsageFolder,
    scanDiskUsageFolder,
    stopDiskUsage,
    openDiskUsage,
    openDiskUsageFolder,
    removeOperationSourcesFromSearchResults,
    removeOperationSourcesFromDiskUsage,
    setOperationCentreVisible,
    toggleOperationCentre,
    showAllOperations,
  };
}
