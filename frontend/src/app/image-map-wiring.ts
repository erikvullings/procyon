import m from 'mithril';
import { toast } from 'mithril-materialized';
import type { FileManagerClient } from '../api/client/file-manager-client';
import { isGeoCandidate } from '../features/image-map/gps-scan';
import { createImageMapController } from '../features/image-map/image-map-controller';
import { ImageMapPanel } from '../features/image-map/image-map-panel';
import { isParentEntry } from '../features/panes/parent-entry';
import { collectMedia } from '../features/player/play-selection';
import { emptySelection, reduceSelection } from '../features/selection/selection';
import { dispatchWorkspaceCommand } from '../features/workspace/dispatch-workspace-command';
import { t } from '../i18n';
import type {
  ActionInvocationContext,
  EntrySummary,
  PaneId,
  TabId,
  WorkspaceProjection,
} from '../models';
import { resolvedTheme } from '../utilities/resolved-theme';
import { workspaceErrorMessage } from './shell-helpers';
/** Sets up the imageMapWiring feature wiring and returns its functions. */
import type { ShellState } from './shell-state';

/** The narrow cross-cutting surface image-map wiring needs from the shell. */
export interface ImageMapCore {
  readonly activeTabKey: (paneId: PaneId) => string;
  readonly selectTab: (client: FileManagerClient, paneId: PaneId, tabId: TabId) => void;
  readonly replaceWorkspace: (next: WorkspaceProjection) => void;
}

export function createImageMapWiring(state: ShellState, core: ImageMapCore) {
  function beginShowOnMap(invocation: ActionInvocationContext | undefined): void {
    const paneId = invocation?.paneId ?? state.workspace?.activePaneId;
    if (state.workspace === undefined || paneId === undefined) return;
    const directory = state.directories.get(core.activeTabKey(paneId));
    const ids = new Set(
      invocation?.selectedEntryIds ??
        state.selections.get(core.activeTabKey(paneId))?.selectedEntryIds ??
        [],
    );
    const chosen = (directory?.entries ?? []).filter(
      (entry) => ids.has(entry.id) && !isParentEntry(entry.id),
    );
    const folderLocation = directory?.location;
    if (
      chosen.length === 0 &&
      (folderLocation === undefined || folderLocation.providerId === 'basket')
    ) {
      toast({ html: t('imageMap', 'noImages') });
      return;
    }
    // Without a selection the folder itself is the source, so every page of it is read.
    const roots: readonly EntrySummary[] =
      chosen.length > 0
        ? chosen
        : [
            {
              id: 'image-map-folder',
              name: '',
              kind: 'directory',
              location: folderLocation,
            } as EntrySummary,
          ];
    openImageMap(
      paneId,
      roots,
      chosen.length === 0 || chosen.some((entry) => entry.kind === 'directory'),
    );
  }

  function imageMapPane(paneId: PaneId): { tabId: TabId; content: m.Children } | undefined {
    const tab = state.imageMapTab;
    if (
      !state.imageMapState.open ||
      state.imageMapController === undefined ||
      state.currentSettings === undefined ||
      tab === undefined ||
      tab.paneId !== paneId
    ) {
      return undefined;
    }
    return {
      tabId: tab.tabId,
      content: m(ImageMapPanel, {
        state: state.imageMapState,
        client: state.client,
        settings: state.currentSettings.map,
        dark: resolvedTheme() === 'dark',
        selectedIds: new Set(
          (state.imageMapPaneId === undefined
            ? undefined
            : state.selections.get(core.activeTabKey(state.imageMapPaneId))
          )?.selectedEntryIds ?? [],
        ),
        onReveal: revealMappedEntry,
        onRecursive: (recursive) => void state.imageMapController?.setRecursive(recursive),
        onCancel: () => state.imageMapController?.cancel(),
        onBeginPlacing: (entry) => state.imageMapController?.beginPlacing(entry),
        onStopPlacing: () => state.imageMapController?.stopPlacing(),
        onPropose: (entry, latitude, longitude) =>
          state.imageMapController?.proposePlacement(entry, latitude, longitude),
        onConfirm: () => void state.imageMapController?.confirmPlacement(),
        onCancelPlacement: () => state.imageMapController?.cancelPlacement(),
      }),
    };
  }

  function ensureImageMapTab(sourcePaneId: PaneId): void {
    const current = state.workspace;
    if (current === undefined) return;
    const existing = state.imageMapTab;
    if (existing !== undefined && current.panesById[existing.paneId]?.tabsById[existing.tabId]) {
      core.selectTab(state.client, existing.paneId, existing.tabId);
      state.focusPane?.(existing.paneId);
      return;
    }
    state.imageMapTab = undefined;
    const targetPaneId = current.paneOrder.find((candidate) => candidate !== sourcePaneId);
    const target = targetPaneId ?? sourcePaneId;
    const sourcePane = current.panesById[sourcePaneId];
    const location = sourcePane?.tabsById[sourcePane.activeTabId]?.location;
    if (location === undefined || current.panesById[target] === undefined) return;
    void dispatchWorkspaceCommand(
      state.client,
      {
        type: 'addTransientTab',
        workspaceId: current.id,
        paneId: target,
        location,
        expectedRevision: current.revision,
      },
      core.replaceWorkspace,
    )
      .then((next) => {
        const tabId = next.panesById[target]?.activeTabId;
        if (tabId !== undefined) {
          state.imageMapTab = { paneId: target, tabId };
          state.focusPane?.(target);
        }
        m.redraw();
      })
      .catch((error: unknown) => {
        toast({ html: workspaceErrorMessage(error, t('imageMap', 'failed', { message: '' })) });
      });
  }

  function openImageMap(paneId: PaneId, roots: readonly EntrySummary[], folder: boolean): void {
    if (state.workspace === undefined) return;
    const pane = state.workspace.panesById[paneId];
    const tab = pane?.tabsById[pane.activeTabId];
    const workspaceId = state.workspace.id;
    state.imageMapPaneId = paneId;
    ensureImageMapTab(paneId);
    state.imageMapController ??= createImageMapController({
      getClient: () => state.client,
      getOpsController: () => state.opsController,
      getState: () => state.imageMapState,
      setState: (next) => {
        state.imageMapState = next;
      },
      toast: (message) => toast({ html: message }),
      redraw: () => m.redraw(),
    });
    void state.imageMapController.open({
      folder,
      collect: async (recursive, signal) => {
        const { entries, truncated } = await collectMedia(roots, {
          client: state.client,
          workspaceId,
          view: {
            ...(tab?.view.sort === undefined ? {} : { sort: tab.view.sort }),
            ...(tab === undefined
              ? {}
              : { showHidden: tab.view.showHidden, foldersFirst: tab.view.foldersFirst }),
          },
          recursive,
          accept: isGeoCandidate,
          signal,
        });
        return { entries, truncated };
      },
    });
  }

  function showEntryOnMap(paneId: PaneId, entry: EntrySummary): void {
    openImageMap(paneId, [entry], false);
  }

  function revealMappedEntry(entry: EntrySummary): void {
    const paneId = state.imageMapPaneId;
    if (paneId === undefined) return;
    const key = core.activeTabKey(paneId);
    const listed = (state.directories.get(key)?.entries ?? []).find(
      (candidate) => candidate.location.uri === entry.location.uri,
    );
    if (listed === undefined) {
      toast({ html: t('imageMap', 'notInFolder', { name: entry.name }) });
      return;
    }
    state.selections.set(
      key,
      reduceSelection(emptySelection, { type: 'selectOnly', entryId: listed.id }, [listed.id]),
    );
    m.redraw();
  }

  return {
    beginShowOnMap,
    imageMapPane,
    ensureImageMapTab,
    openImageMap,
    showEntryOnMap,
    revealMappedEntry,
  };
}
