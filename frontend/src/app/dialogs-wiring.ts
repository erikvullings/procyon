import m from 'mithril';
import { toast } from 'mithril-materialized';
import type { FileManagerClient } from '../api/client/file-manager-client';
import { CHANGE_ATTRIBUTES_ACTION_ID } from '../features/attributes/attribute-form';
import {
  COMPARE_BY_CONTENT_ACTION_ID,
  COMPARE_FILES_ACTION_ID,
  resolveDiffPair,
} from '../features/file-diff/file-diff-actions';
import { CONVERT_GEO_ACTION_ID, isConvertibleGeoFile } from '../features/geo/convert-geo-actions';
import { MINIFY_GEO_ACTION_ID } from '../features/geo/minify-geo-actions';
import { SHOW_ON_MAP_ACTION_ID } from '../features/image-map/image-map-actions';
import {
  EDIT_IMAGE_METADATA_ACTION_ID,
  isMetadataEditable,
  supportsRichFields,
} from '../features/image-metadata/image-metadata-edit';
import type { PaneDirectoryView } from '../features/navigation/navigation';
import { parentLocation } from '../features/navigation/navigation';
import type { QueueMode } from '../features/player/play-selection';
import {
  collectMedia,
  MAX_QUEUED_TRACKS,
  queueItemsFromEntries,
  queueTitle,
} from '../features/player/play-selection';
import {
  COMPARE_SNAPSHOT_ACTION_ID,
  FIND_BROKEN_LINKS_ACTION_ID,
  FIND_MUSIC_DUPLICATES_ACTION_ID,
  MEDIA_QUEUE_ACTIONS,
  OPEN_LINK_AT_CURSOR_ACTION_ID,
  REVEAL_LINK_AT_CURSOR_ACTION_ID,
  SAVE_PLAYLIST_ACTION_ID,
  SAVE_SNAPSHOT_ACTION_ID,
} from '../features/player/player-actions';
import type { PlaylistExportFormat, PlaylistPathStyle } from '../features/playlist/playlist-writer';
import { writePlaylist } from '../features/playlist/playlist-writer';
import { defaultSnapshotName, isSnapshotFileName } from '../features/snapshots/snapshot-state';
import {
  COMBINE_FILES_ACTION_ID,
  isCombinablePart,
  isSplittable,
  SPLIT_FILE_ACTION_ID,
} from '../features/split/split-actions';
import { SYNC_PROFILES_ACTION_ID } from '../features/sync-profiles/sync-profile-actions';
import { errorText, t } from '../i18n';
import type {
  ActionInvocationContext,
  ChangeAttributes,
  EntrySummary,
  GeoTransform,
  ImageMetadataEdit,
  Location,
  PaneId,
  SplitFile,
  TabId,
  WorkspaceProjection,
} from '../models';
/** Sets up the dialogWiring feature wiring and returns its functions. */
import type { ShellState } from './shell-state';

/** The narrow cross-cutting surface dialog/action wiring needs from the shell. */
export interface DialogCore {
  readonly activeTabKey: (paneId: PaneId) => string;
  readonly refetchAffectedPanes: (
    paneId?: PaneId,
    options?: {
      readonly background?: boolean;
      readonly settleMs?: number;
      readonly force?: boolean;
    },
  ) => void;
  readonly selectTab: (client: FileManagerClient, paneId: PaneId, tabId: TabId) => void;
  readonly replaceWorkspace: (next: WorkspaceProjection) => void;
  readonly activatePane: (client: FileManagerClient, paneId: PaneId) => Promise<void>;
  readonly actOnLinkAtCursor: (mode: 'open' | 'reveal') => void;
  readonly beginShowOnMap: (invocation: ActionInvocationContext | undefined) => void;
}

export function createDialogWiring(shellState: ShellState, core: DialogCore) {
  function beginSaveSnapshot(invocation: ActionInvocationContext | undefined): void {
    const paneId = invocation?.paneId ?? shellState.workspace?.activePaneId;
    if (paneId === undefined) return;
    const directory = shellState.directories.get(core.activeTabKey(paneId));
    const ids = new Set(
      invocation?.selectedEntryIds ??
        shellState.selections.get(core.activeTabKey(paneId))?.selectedEntryIds ??
        [],
    );
    const selectedFolders = (directory?.entries ?? []).filter(
      (entry) => ids.has(entry.id) && entry.kind === 'directory',
    );
    const folder = selectedFolders.length === 1 ? selectedFolders[0] : undefined;
    const root =
      folder?.location ??
      (directory?.location?.providerId === 'basket' ? undefined : directory?.location);
    if (root === undefined) {
      toast({ html: t('snapshots', 'selectFolder') });
      return;
    }
    const name =
      folder?.name ?? decodeURIComponent(root.uri.replace(/\/+$/u, '').split('/').pop() ?? '');
    shellState.saveSnapshotRequest = {
      root,
      directory: parentLocation(root),
      defaultName: defaultSnapshotName(name),
    };
    m.redraw();
  }

  function beginSnapshotComparison(invocation: ActionInvocationContext | undefined): void {
    const paneId = invocation?.paneId ?? shellState.workspace?.activePaneId;
    if (
      shellState.workspace === undefined ||
      paneId === undefined ||
      shellState.snapshotController === undefined
    )
      return;
    const directory = shellState.directories.get(core.activeTabKey(paneId));
    const ids = new Set(
      invocation?.selectedEntryIds ??
        shellState.selections.get(core.activeTabKey(paneId))?.selectedEntryIds ??
        [],
    );
    const snapshot = (directory?.entries ?? []).find(
      (entry) => ids.has(entry.id) && entry.kind === 'file' && isSnapshotFileName(entry.name),
    );
    if (snapshot === undefined) {
      toast({ html: t('snapshots', 'selectSnapshot') });
      return;
    }
    const otherPaneId = shellState.workspace.paneOrder.find((candidate) => candidate !== paneId);
    const target =
      (otherPaneId === undefined
        ? undefined
        : shellState.directories.get(core.activeTabKey(otherPaneId))?.location) ??
      directory?.location;
    if (target === undefined || target.providerId === 'basket') {
      toast({ html: t('snapshots', 'noTarget') });
      return;
    }
    void shellState.snapshotController.start({
      type: 'compare',
      snapshot: snapshot.location,
      target,
    });
  }

  function beginLinkDoctorScan(invocation: ActionInvocationContext | undefined): void {
    const paneId = invocation?.paneId ?? shellState.workspace?.activePaneId;
    if (shellState.linkDoctorController === undefined || paneId === undefined) return;
    const directory = shellState.directories.get(core.activeTabKey(paneId));
    const ids = new Set(
      invocation?.selectedEntryIds ??
        shellState.selections.get(core.activeTabKey(paneId))?.selectedEntryIds ??
        [],
    );
    const folders = (directory?.entries ?? [])
      .filter((entry) => ids.has(entry.id) && entry.kind === 'directory')
      .map((entry) => entry.location);
    const roots =
      folders.length > 0
        ? folders
        : directory?.location === undefined || directory.location.providerId === 'basket'
          ? []
          : [directory.location];
    if (roots.length === 0) {
      toast({ html: t('linkDoctor', 'noFolder') });
      return;
    }
    void shellState.linkDoctorController.start(roots);
  }

  function beginMusicDuplicateScan(invocation: ActionInvocationContext | undefined): void {
    const paneId = invocation?.paneId ?? shellState.workspace?.activePaneId;
    if (shellState.musicDuplicatesController === undefined || paneId === undefined) return;
    const directory = shellState.directories.get(core.activeTabKey(paneId));
    const ids = new Set(
      invocation?.selectedEntryIds ??
        shellState.selections.get(core.activeTabKey(paneId))?.selectedEntryIds ??
        [],
    );
    const folders = (directory?.entries ?? [])
      .filter((entry) => ids.has(entry.id) && entry.kind === 'directory')
      .map((entry) => entry.location);
    const roots =
      folders.length > 0
        ? folders
        : directory?.location === undefined || directory.location.providerId === 'basket'
          ? []
          : [directory.location];
    if (roots.length === 0) {
      toast({ html: t('musicDuplicates', 'noFolder') });
      return;
    }
    void shellState.musicDuplicatesController.start(roots);
  }

  function beginSavePlaylist(invocation: ActionInvocationContext | undefined): void {
    const paneId = invocation?.paneId ?? shellState.workspace?.activePaneId;
    if (shellState.workspace === undefined || paneId === undefined) return;
    const directory = shellState.directories.get(core.activeTabKey(paneId));
    const selection = shellState.selections.get(core.activeTabKey(paneId));
    const ids = new Set(invocation?.selectedEntryIds ?? selection?.selectedEntryIds ?? []);
    const inBasket = directory?.location?.providerId === 'basket';
    const chosen = (directory?.entries ?? []).filter((entry) =>
      ids.size > 0 ? ids.has(entry.id) : inBasket,
    );
    const writable = (
      view: PaneDirectoryView | undefined,
    ): view is PaneDirectoryView & {
      readonly location: Location;
    } =>
      view?.location !== undefined &&
      view.location.providerId !== 'basket' &&
      view.writable !== false;
    const otherPaneId = shellState.workspace.paneOrder.find((candidate) => candidate !== paneId);
    const target = writable(directory)
      ? directory
      : otherPaneId === undefined
        ? undefined
        : shellState.directories.get(core.activeTabKey(otherPaneId));
    if (!writable(target)) {
      toast({ html: t('playlistExport', 'noDestination') });
      return;
    }
    const pane = shellState.workspace.panesById[paneId];
    const tab = pane?.tabsById[pane.activeTabId];
    void collectMedia(chosen, {
      client: shellState.client,
      workspaceId: shellState.workspace.id,
      view: {
        ...(tab?.view.sort === undefined ? {} : { sort: tab.view.sort }),
        ...(tab === undefined
          ? {}
          : { showHidden: tab.view.showHidden, foldersFirst: tab.view.foldersFirst }),
      },
      recursive: false,
    })
      .then(({ entries }) => {
        if (entries.length === 0) {
          toast({ html: t('playlistExport', 'nothingToSave') });
          return;
        }
        shellState.savePlaylistRequest = {
          tracks: entries.map((entry) => ({
            location: entry.location,
            title: queueTitle(entry),
            durationSeconds: undefined,
          })),
          directory: target.location,
        };
        m.redraw();
      })
      .catch((error: unknown) => {
        toast({ html: errorText(error) });
      });
  }

  function beginMinifyGeo(invocation: ActionInvocationContext | undefined): void {
    const paneId = invocation?.paneId ?? shellState.workspace?.activePaneId;
    if (paneId === undefined) return;
    const ids = new Set(
      invocation?.selectedEntryIds ??
        shellState.selections.get(core.activeTabKey(paneId))?.selectedEntryIds ??
        [],
    );
    const files = (shellState.directories.get(core.activeTabKey(paneId))?.entries ?? []).filter(
      (entry) => ids.has(entry.id) && isConvertibleGeoFile(entry),
    );
    if (files.length === 0) {
      toast({ html: t('geoConvert', 'noFiles') });
      return;
    }
    shellState.minifyGeoRequest = {
      files: files.map((entry) => ({ location: entry.location, name: entry.name })),
    };
    m.redraw();
  }

  function confirmMinifyGeo(transform: GeoTransform): void {
    const request = shellState.minifyGeoRequest;
    shellState.minifyGeoRequest = undefined;
    if (request === undefined) return;
    void shellState.opsController
      .transformGeo(
        request.files.map((file) => file.location),
        transform,
      )
      .then(() => {
        toast({ html: t('geoMinify', 'started', request.files.length) });
        m.redraw();
      })
      .catch((error: unknown) => {
        toast({
          html: t('geoMinify', 'failed', {
            message: errorText(error),
          }),
        });
      });
    m.redraw();
  }

  function selectionWithOtherFolder(
    invocation: ActionInvocationContext | undefined,
  ):
    | { readonly selected: readonly EntrySummary[]; readonly otherPaneFolder?: Location }
    | undefined {
    const paneId = invocation?.paneId ?? shellState.workspace?.activePaneId;
    if (paneId === undefined) return undefined;
    const ids = new Set(
      invocation?.selectedEntryIds ??
        shellState.selections.get(core.activeTabKey(paneId))?.selectedEntryIds ??
        [],
    );
    const selected = (shellState.directories.get(core.activeTabKey(paneId))?.entries ?? []).filter(
      (entry) => ids.has(entry.id),
    );
    const otherPaneId = shellState.workspace?.paneOrder.find((candidate) => candidate !== paneId);
    const other =
      otherPaneId === undefined
        ? undefined
        : shellState.directories.get(core.activeTabKey(otherPaneId))?.location;
    return other === undefined || other.providerId === 'basket'
      ? { selected }
      : { selected, otherPaneFolder: other };
  }

  function selectedEntriesOf(
    paneId: PaneId,
    explicitIds?: readonly string[],
  ): readonly EntrySummary[] {
    const ids = new Set(
      explicitIds ?? shellState.selections.get(core.activeTabKey(paneId))?.selectedEntryIds ?? [],
    );
    return (shellState.directories.get(core.activeTabKey(paneId))?.entries ?? []).filter((entry) =>
      ids.has(entry.id),
    );
  }

  function beginCompareFiles(invocation: ActionInvocationContext | undefined): void {
    const paneId = invocation?.paneId ?? shellState.workspace?.activePaneId;
    if (paneId === undefined) return;
    const otherPaneId = shellState.workspace?.paneOrder.find((candidate) => candidate !== paneId);
    const pair = resolveDiffPair(
      selectedEntriesOf(paneId, invocation?.selectedEntryIds),
      otherPaneId === undefined ? [] : selectedEntriesOf(otherPaneId),
      shellState.workspace?.paneOrder[0] === paneId,
    );
    if (pair === undefined) {
      toast({ html: t('fileDiff', 'selectTwoFiles') });
      return;
    }
    shellState.fileDiffRequest = {
      left: { location: pair.left.location, name: pair.left.name },
      right: { location: pair.right.location, name: pair.right.name },
    };
    m.redraw();
  }

  function beginSplitFile(invocation: ActionInvocationContext | undefined): void {
    const context = selectionWithOtherFolder(invocation);
    const files = (context?.selected ?? []).filter(isSplittable);
    if (context === undefined || files.length === 0) {
      toast({ html: t('splitCombine', 'noSelection') });
      return;
    }
    shellState.splitFileRequest = {
      files: files.map((entry) => ({
        location: entry.location,
        name: entry.name,
        size: entry.size ?? 0,
      })),
      ...(context.otherPaneFolder === undefined
        ? {}
        : { otherPaneFolder: context.otherPaneFolder }),
    };
    m.redraw();
  }

  function confirmSplitFile(
    split: SplitFile,
    destination: Location | undefined,
    count: number,
  ): void {
    const request = shellState.splitFileRequest;
    shellState.splitFileRequest = undefined;
    if (request === undefined) return;
    void shellState.opsController
      .splitFile(
        request.files.map((file) => file.location),
        split,
        destination,
      )
      .then(() => {
        toast({ html: t('splitCombine', 'splitStarted', count) });
        m.redraw();
      })
      .catch((error: unknown) => {
        toast({
          html: t('splitCombine', 'failed', {
            message: errorText(error),
          }),
        });
      });
    m.redraw();
  }

  function beginCombineFiles(invocation: ActionInvocationContext | undefined): void {
    const context = selectionWithOtherFolder(invocation);
    const parts = (context?.selected ?? []).filter(isCombinablePart);
    if (context === undefined || parts.length === 0) {
      toast({ html: t('splitCombine', 'noParts') });
      return;
    }
    shellState.combineFilesRequest = {
      parts: parts.map((entry) => entry.location),
      ...(context.otherPaneFolder === undefined
        ? {}
        : { otherPaneFolder: context.otherPaneFolder }),
    };
    m.redraw();
  }

  function confirmCombineFiles(destination: Location | undefined, name: string | undefined): void {
    const request = shellState.combineFilesRequest;
    shellState.combineFilesRequest = undefined;
    if (request === undefined) return;
    void shellState.opsController
      .combineFiles(request.parts, destination, name)
      .then(() => {
        toast({ html: t('splitCombine', 'combineStarted') });
        m.redraw();
      })
      .catch((error: unknown) => {
        toast({
          html: t('splitCombine', 'failed', {
            message: errorText(error),
          }),
        });
      });
    m.redraw();
  }

  function beginConvertGeo(invocation: ActionInvocationContext | undefined): void {
    const paneId = invocation?.paneId ?? shellState.workspace?.activePaneId;
    if (paneId === undefined) return;
    const ids = new Set(
      invocation?.selectedEntryIds ??
        shellState.selections.get(core.activeTabKey(paneId))?.selectedEntryIds ??
        [],
    );
    const files = (shellState.directories.get(core.activeTabKey(paneId))?.entries ?? []).filter(
      (entry) => ids.has(entry.id) && isConvertibleGeoFile(entry),
    );
    if (files.length === 0) {
      toast({ html: t('geoConvert', 'noFiles') });
      return;
    }
    shellState.convertGeoRequest = {
      files: files.map((entry) => ({ location: entry.location, name: entry.name })),
    };
    m.redraw();
  }

  function confirmConvertGeo(transform: GeoTransform): void {
    const request = shellState.convertGeoRequest;
    shellState.convertGeoRequest = undefined;
    if (request === undefined) return;
    void shellState.opsController
      .transformGeo(
        request.files.map((file) => file.location),
        transform,
      )
      .then(() => {
        toast({ html: t('geoConvert', 'started', request.files.length) });
        m.redraw();
      })
      .catch((error: unknown) => {
        toast({
          html: t('geoConvert', 'failed', {
            message: errorText(error),
          }),
        });
      });
    m.redraw();
  }

  function beginChangeAttributes(invocation: ActionInvocationContext | undefined): void {
    const paneId = invocation?.paneId ?? shellState.workspace?.activePaneId;
    if (paneId === undefined) return;
    const ids = new Set(
      invocation?.selectedEntryIds ??
        shellState.selections.get(core.activeTabKey(paneId))?.selectedEntryIds ??
        [],
    );
    const selected = (shellState.directories.get(core.activeTabKey(paneId))?.entries ?? []).filter(
      (entry) => ids.has(entry.id),
    );
    openChangeAttributes(selected);
  }

  function openChangeAttributes(entries: readonly EntrySummary[]): void {
    if (entries.length === 0) {
      toast({ html: t('attributes', 'noSelection') });
      return;
    }
    shellState.changeAttributesRequest = { files: entries.map((entry) => entry.location) };
    m.redraw();
  }

  function confirmChangeAttributes(change: ChangeAttributes, count: number): void {
    const request = shellState.changeAttributesRequest;
    shellState.changeAttributesRequest = undefined;
    if (request === undefined) return;
    void shellState.opsController
      .changeAttributes(request.files, change)
      .then(() => {
        toast({ html: t('attributes', 'started', count) });
        m.redraw();
      })
      .catch((error: unknown) => {
        toast({
          html: t('attributes', 'failed', {
            message: errorText(error),
          }),
        });
      });
    m.redraw();
  }

  function beginEditImageMetadata(invocation: ActionInvocationContext | undefined): void {
    const paneId = invocation?.paneId ?? shellState.workspace?.activePaneId;
    if (paneId === undefined) return;
    const ids = new Set(
      invocation?.selectedEntryIds ??
        shellState.selections.get(core.activeTabKey(paneId))?.selectedEntryIds ??
        [],
    );
    const images = (shellState.directories.get(core.activeTabKey(paneId))?.entries ?? []).filter(
      (entry) => ids.has(entry.id) && isMetadataEditable(entry),
    );
    if (images.length === 0) {
      toast({ html: t('imageEdit', 'noEditableImages') });
      return;
    }
    shellState.editImageMetadataRequest = {
      files: images.map((entry) => entry.location),
      richFields: supportsRichFields(images),
    };
    m.redraw();
  }

  function confirmEditImageMetadata(edit: ImageMetadataEdit): void {
    const request = shellState.editImageMetadataRequest;
    shellState.editImageMetadataRequest = undefined;
    if (request === undefined) return;
    void shellState.opsController
      .editImageMetadata(request.files, edit)
      .then(() => {
        toast({ html: t('imageEdit', 'started', request.files.length) });
        m.redraw();
      })
      .catch((error: unknown) => {
        toast({
          html: t('imageEdit', 'failed', {
            message: errorText(error),
          }),
        });
      });
    m.redraw();
  }

  function confirmSavePlaylist(
    fileName: string,
    format: PlaylistExportFormat,
    pathStyle: PlaylistPathStyle,
  ): void {
    const request = shellState.savePlaylistRequest;
    shellState.savePlaylistRequest = undefined;
    if (request === undefined) return;
    const written = writePlaylist(format, request.tracks, request.directory, pathStyle);
    void shellState.opsController
      .createTextFile(request.directory, fileName, written.content)
      .then(() => {
        toast({ html: t('playlistExport', 'saved', { name: fileName }) });
        if (written.fellBackToAbsolute) toast({ html: t('playlistExport', 'fellBackToAbsolute') });
        for (const paneId of shellState.workspace?.paneOrder ?? [])
          core.refetchAffectedPanes(paneId);
        m.redraw();
      })
      .catch((error: unknown) => {
        toast({
          html: t('playlistExport', 'failed', {
            message: errorText(error),
          }),
        });
      });
    m.redraw();
  }

  function queueSelection(
    mode: QueueMode,
    recursive: boolean,
    invocation: ActionInvocationContext | undefined,
  ): void {
    const player = shellState.playerController;
    const paneId = invocation?.paneId ?? shellState.workspace?.activePaneId;
    if (player === undefined || shellState.workspace === undefined || paneId === undefined) return;
    const directory = shellState.directories.get(core.activeTabKey(paneId));
    const ids = new Set(
      invocation?.selectedEntryIds ??
        shellState.selections.get(core.activeTabKey(paneId))?.selectedEntryIds ??
        [],
    );
    const cursorId =
      invocation?.cursorEntryId ??
      shellState.selections.get(core.activeTabKey(paneId))?.cursorEntryId;
    const chosen = (directory?.entries ?? []).filter((entry) =>
      ids.size > 0 ? ids.has(entry.id) : entry.id === cursorId,
    );
    const pane = shellState.workspace.panesById[paneId];
    const tab = pane?.tabsById[pane.activeTabId];
    void collectMedia(chosen, {
      client: shellState.client,
      workspaceId: shellState.workspace.id,
      view: {
        ...(tab?.view.sort === undefined ? {} : { sort: tab.view.sort }),
        ...(tab === undefined
          ? {}
          : { showHidden: tab.view.showHidden, foldersFirst: tab.view.foldersFirst }),
      },
      recursive,
    })
      .then(({ entries, truncated }) => {
        if (entries.length === 0) {
          toast({ html: t('player', 'noPlayableTracks') });
          return;
        }
        const items = queueItemsFromEntries(entries);
        if (mode === 'play') player.playItems(items);
        else if (mode === 'next') player.enqueueNext(items);
        else player.enqueue(items);
        if (mode !== 'play') toast({ html: t('player', 'queued', items.length) });
        if (truncated) toast({ html: t('player', 'truncatedQueue', { n: MAX_QUEUED_TRACKS }) });
        m.redraw();
      })
      .catch((error: unknown) => {
        toast({ html: errorText(error) });
      });
  }

  function invokeClientAction(actionId: string, invocation?: ActionInvocationContext): boolean {
    const player = shellState.playerController;
    if (player === undefined) return false;
    if (actionId === SAVE_PLAYLIST_ACTION_ID) {
      beginSavePlaylist(invocation);
      return true;
    }
    if (actionId === MINIFY_GEO_ACTION_ID) {
      beginMinifyGeo(invocation);
      return true;
    }
    if (actionId === CONVERT_GEO_ACTION_ID) {
      beginConvertGeo(invocation);
      return true;
    }
    if (actionId === COMPARE_FILES_ACTION_ID) {
      beginCompareFiles(invocation);
      return true;
    }
    if (actionId === SYNC_PROFILES_ACTION_ID) {
      shellState.syncProfilesDialogOpen = true;
      return true;
    }
    if (actionId === COMPARE_BY_CONTENT_ACTION_ID) {
      shellState.comparisonController.startComparison('content');
      return true;
    }
    if (actionId === SPLIT_FILE_ACTION_ID) {
      beginSplitFile(invocation);
      return true;
    }
    if (actionId === COMBINE_FILES_ACTION_ID) {
      beginCombineFiles(invocation);
      return true;
    }
    if (actionId === SHOW_ON_MAP_ACTION_ID) {
      core.beginShowOnMap(invocation);
      return true;
    }
    if (actionId === EDIT_IMAGE_METADATA_ACTION_ID) {
      beginEditImageMetadata(invocation);
      return true;
    }
    if (actionId === CHANGE_ATTRIBUTES_ACTION_ID) {
      beginChangeAttributes(invocation);
      return true;
    }
    if (actionId === FIND_MUSIC_DUPLICATES_ACTION_ID) {
      beginMusicDuplicateScan(invocation);
      return true;
    }
    if (
      actionId === OPEN_LINK_AT_CURSOR_ACTION_ID ||
      actionId === REVEAL_LINK_AT_CURSOR_ACTION_ID
    ) {
      core.actOnLinkAtCursor(actionId === OPEN_LINK_AT_CURSOR_ACTION_ID ? 'open' : 'reveal');
      return true;
    }
    if (actionId === SAVE_SNAPSHOT_ACTION_ID) {
      beginSaveSnapshot(invocation);
      return true;
    }
    if (actionId === COMPARE_SNAPSHOT_ACTION_ID) {
      beginSnapshotComparison(invocation);
      return true;
    }
    if (actionId === FIND_BROKEN_LINKS_ACTION_ID) {
      beginLinkDoctorScan(invocation);
      return true;
    }
    const queueAction = MEDIA_QUEUE_ACTIONS.find((candidate) => candidate.id === actionId);
    if (queueAction !== undefined) {
      queueSelection(queueAction.mode, queueAction.recursive, invocation);
      return true;
    }
    if (actionId === 'client.playerTogglePlay') player.togglePlay();
    else if (actionId === 'client.playerNext') player.next();
    else if (actionId === 'client.playerPrevious') player.previous();
    else return false;
    m.redraw();
    return true;
  }

  return {
    beginSaveSnapshot,
    beginSnapshotComparison,
    beginLinkDoctorScan,
    beginMusicDuplicateScan,
    beginSavePlaylist,
    beginMinifyGeo,
    confirmMinifyGeo,
    selectionWithOtherFolder,
    selectedEntriesOf,
    beginCompareFiles,
    beginSplitFile,
    confirmSplitFile,
    beginCombineFiles,
    confirmCombineFiles,
    beginConvertGeo,
    confirmConvertGeo,
    beginChangeAttributes,
    openChangeAttributes,
    confirmChangeAttributes,
    beginEditImageMetadata,
    confirmEditImageMetadata,
    confirmSavePlaylist,
    queueSelection,
    invokeClientAction,
  };
}
