import type { FileManagerClient } from '../../api/client/file-manager-client';
import { t } from '../../i18n';
import type {
  ActionDescriptor,
  ActionInvocationContext,
  EntrySummary,
  Location,
  PaneId,
  Settings,
  WorkspaceProjection,
} from '../../models';
import type { ClipboardState } from '../clipboard/clipboard';
import { clearClipboard } from '../clipboard/clipboard';
import {
  copySelectionToClipboard,
  isCopySelectionAction,
} from '../clipboard/copy-selection-actions';
import type { CommandAvailabilityContext } from '../commands/availability';
import { evaluateActionAvailability } from '../commands/availability';
import type { ArchiveCreateRequest } from '../dialogs/dialog-ui-controller';
import type { NavigationController, PaneDirectoryView } from '../navigation/navigation';
import type { OperationsController } from '../operations/operations-controller';
import { isParentEntry } from '../panes/parent-entry';
import type { SelectionState } from '../selection/selection';

/** Context required by ActionCommandController for state access and dependencies. */
export interface ActionCommandControllerContext {
  // State getters
  getCommandPaletteOpen(): boolean;
  getContextMenu():
    | {
        readonly paneId: PaneId;
        readonly entries: readonly EntrySummary[];
        readonly x: number;
        readonly y: number;
      }
    | undefined;
  getCommandPaletteRecency(): Map<string, number>;

  // State setters
  setCommandPaletteOpen(open: boolean): void;
  setContextMenu(
    menu:
      | {
          readonly paneId: PaneId;
          readonly entries: readonly EntrySummary[];
          readonly x: number;
          readonly y: number;
        }
      | undefined,
  ): void;

  // Dependencies
  getActiveDirectory(): { paneId: PaneId; location: Location } | undefined;
  getActiveTabKey(paneId: PaneId): string;
  getSelections(): Map<string, SelectionState>;
  getDirectories(): Map<string, PaneDirectoryView>;
  getCurrentSettings(): Settings | undefined;
  getClient(): FileManagerClient;
  getRegisteredActions(): readonly ActionDescriptor[];
  getWorkspace(): WorkspaceProjection | undefined;
  getNavigation(): NavigationController;
  getOpsController(): OperationsController;
  getGetSelectedEntries(): (
    selection: SelectionState | undefined,
    entries: readonly EntrySummary[],
  ) => readonly EntrySummary[];
  getClipboard(): ClipboardState;
  replaceClipboard(next?: ClipboardState): void;
  toast(options: { html: string }): void;
  getOpenTerminalSupported(): boolean;
  openCreateDirectory(location?: import('../../models').Location): void;
  setArchiveCreateRequest(request: ArchiveCreateRequest): void;
  openFinderTagsDialog(
    request: import('../dialogs/dialog-ui-controller').FinderTagsDialogRequest,
  ): void;
  openSpotlightCommentDialog(
    request: import('../dialogs/dialog-ui-controller').SpotlightCommentDialogRequest,
  ): void;
  /** Starts a checksum job over the active pane's selection (task 0077). */
  calculateChecksums(): void;
  /** Starts a duplicate scan rooted at the active pane's directory (task 0077). */
  findDuplicates(): void;
  /** Opens a disk-usage treemap tab for the active local directory. */
  openDiskUsage(): void;
  /** Opens the Properties dialog for the active pane's selection (task 0140). */
  openPropertiesForActivePane(): void;
  /** Scans `entry`'s well-known related-file locations and opens the review checklist before
   * anything is deleted (task 0148's macOS application uninstaller). */
  uninstallApplication(paneId: PaneId, entry: EntrySummary): void;
  /** Toggles the directory-tree sidebar (task 0139). */
  toggleDirectoryTree(): void;
  redraw(): void;
}

/** Controller interface for action and command invocation. */
export interface ActionCommandController {
  /**
   * Gets the current action invocation context based on the active pane and selections.
   */
  actionContext(): ActionInvocationContext;

  /**
   * Gets the command availability context for evaluating which actions are available.
   */
  commandAvailabilityContext(
    selectedEntries?: readonly EntrySummary[],
    paneId?: PaneId,
  ): CommandAvailabilityContext;

  /**
   * Builds platform-specific parameters (e.g., `{ uri }`) for certain core actions.
   */
  platformActionParameters(
    actionId: string,
    selectedEntries: readonly EntrySummary[],
    directoryLocation: Location | undefined,
  ): { uri: string } | undefined;

  /**
   * Invokes an action by ID, updating recency and handling errors.
   */
  invokeActionById(actionId: string, parameters: unknown, context: ActionInvocationContext): void;

  /**
   * Invokes an action from the command palette, with special handling for favorites, copy, etc.
   */
  invokePaletteAction(
    action: ActionDescriptor,
    parameters?: unknown,
    context?: ActionInvocationContext,
  ): void;

  /**
   * Opens the context menu at the given position for the specified entries.
   */
  openContextMenu(paneId: PaneId, entries: readonly EntrySummary[], x: number, y: number): void;

  /**
   * Invokes an action from the context menu, with special handling for paste, refresh, etc.
   */
  invokeContextMenuAction(actionId: string): void;
}

/**
 * Factory function to create an ActionCommandController.
 */
export function createActionCommandController(
  context: ActionCommandControllerContext,
): ActionCommandController {
  function actionContext(): ActionInvocationContext {
    const active = context.getActiveDirectory();
    const selection =
      active === undefined
        ? undefined
        : context.getSelections().get(context.getActiveTabKey(active.paneId));
    const selectedEntryIds = (selection?.selectedEntryIds ?? []).filter(
      (entryId) => !isParentEntry(entryId),
    );
    return {
      ...(active === undefined ? {} : { paneId: active.paneId }),
      ...(selectedEntryIds.length === 0 ? {} : { selectedEntryIds }),
      ...(selection?.cursorEntryId === undefined ? {} : { cursorEntryId: selection.cursorEntryId }),
    };
  }

  function commandAvailabilityContext(
    selectedEntries?: readonly EntrySummary[],
    paneId?: PaneId,
  ): CommandAvailabilityContext {
    const active = context.getActiveDirectory();
    const effectivePaneId = paneId ?? active?.paneId;
    const effectiveKey =
      effectivePaneId === undefined ? undefined : context.getActiveTabKey(effectivePaneId);
    const effectiveEntries =
      selectedEntries ??
      (effectiveKey === undefined
        ? []
        : context.getGetSelectedEntries()(
            context.getSelections().get(effectiveKey),
            context.getDirectories().get(effectiveKey)?.entries ?? [],
          ));
    const directory =
      effectiveKey === undefined ? undefined : context.getDirectories().get(effectiveKey);
    return {
      selectedEntries: effectiveEntries,
      locationWritable: directory?.writable === true,
      clipboardHasEntries: context.getClipboard().locations.length > 0,
      openTerminalSupported: context.getOpenTerminalSupported(),
    };
  }

  function platformActionParameters(
    actionId: string,
    selectedEntries: readonly EntrySummary[],
    directoryLocation: Location | undefined,
  ): { uri: string } | undefined {
    if (
      actionId === 'core.open' ||
      actionId === 'core.view' ||
      actionId === 'core.edit' ||
      actionId === 'core.openWith' ||
      actionId === 'core.quickLook' ||
      actionId === 'core.revealInSystemFileManager'
    ) {
      const entry = selectedEntries[0];
      return entry === undefined ? undefined : { uri: entry.location.uri };
    }
    if (actionId === 'core.openTerminal') {
      return directoryLocation === undefined ? undefined : { uri: directoryLocation.uri };
    }
    return undefined;
  }

  function invokeActionById(
    actionId: string,
    parameters: unknown,
    actionContext: ActionInvocationContext,
  ): void {
    void context
      .getClient()
      .invokeAction({
        actionId,
        ...(parameters === undefined ? {} : { parameters }),
        context: actionContext,
      })
      .then(() => {
        context.getCommandPaletteRecency().set(actionId, Date.now());
        context.redraw();
      })
      .catch((error: unknown) => {
        context.toast({
          html: error instanceof Error ? error.message : t('action', 'unableToRun'),
        });
        context.redraw();
      });
  }

  /** Handles `core.editFinderTags`/`core.editSpotlightComment` (task 0136), shared by the context
   * menu and command palette dispatch paths: both fetch the entry's current tags/comment before
   * opening the pre-filled editor dialog, then let the dialog's own Save button perform the write
   * through `setFinderTags`/`setSpotlightComment` - this only opens the dialog. Returns whether it
   * handled `actionId`, so callers fall through to their own dispatch otherwise. */
  function openEntryMetadataDialog(actionId: string, entry: EntrySummary | undefined): boolean {
    if (actionId === 'core.editFinderTags') {
      if (entry === undefined) return true;
      void context
        .getClient()
        .getFinderTags(entry.location.uri)
        .then((current) => {
          context.openFinderTagsDialog({ entry, tags: current?.tags ?? [] });
          context.redraw();
        });
      return true;
    }
    if (actionId === 'core.editSpotlightComment') {
      if (entry === undefined) return true;
      void context
        .getClient()
        .getSpotlightComment(entry.location.uri)
        .then((current) => {
          context.openSpotlightCommentDialog({ entry, comment: current?.comment ?? '' });
          context.redraw();
        });
      return true;
    }
    return false;
  }

  function invokePaletteAction(
    action: ActionDescriptor,
    parameters?: unknown,
    contextParam = actionContext(),
  ): void {
    if (action.id === 'core.palette') return;
    if (action.id === 'core.favourites') {
      context.setCommandPaletteOpen(true);
      return;
    }
    if (action.id.startsWith('core.favourite.')) {
      const index = Number(action.id.slice('core.favourite.'.length));
      const favourite = context.getCurrentSettings()?.favouriteLocations[index];
      if (favourite !== undefined && contextParam.paneId !== undefined) {
        void context.getNavigation().navigate(contextParam.paneId, favourite.location);
      }
      return;
    }
    if (action.id === 'core.createDirectory') {
      context.openCreateDirectory(undefined);
      return;
    }
    // These three actions carry no default keybinding by design (spec §18/§35, tasks 0077/0140)
    // - the command palette is their only entry point - so unlike keybinding dispatch they must be
    // special-cased here too, rather than falling through to the generic `invokeActionById`, which
    // hits the backend's synchronous action-invoke endpoint and can't drive their streamed job/
    // dialog flows.
    if (action.id === 'core.calculateChecksum') {
      context.calculateChecksums();
      return;
    }
    if (action.id === 'core.findDuplicates') {
      context.findDuplicates();
      return;
    }
    if (action.id === 'core.showProperties') {
      context.openPropertiesForActivePane();
      return;
    }
    if (action.id === 'client.toggleDirectoryTree') {
      context.toggleDirectoryTree();
      return;
    }
    if (action.id === 'client.diskUsage') {
      context.openDiskUsage();
      return;
    }
    const paneId = contextParam.paneId;
    const directory =
      paneId === undefined
        ? undefined
        : context.getDirectories().get(context.getActiveTabKey(paneId));
    const selectedEntries =
      directory === undefined || contextParam.selectedEntryIds === undefined
        ? []
        : directory.entries.filter((entry) => new Set(contextParam.selectedEntryIds).has(entry.id));
    if (openEntryMetadataDialog(action.id, selectedEntries[0])) return;
    // Discovery-then-review-dialog flow (task 0148), like calculateChecksum/findDuplicates above:
    // the generic `invokeActionById` fallthrough only hits the backend's synchronous action-invoke
    // endpoint, which can't drive this action's discovery request + checklist dialog.
    if (action.id === 'core.uninstallApplication') {
      const bundle = selectedEntries[0];
      if (paneId !== undefined && bundle !== undefined)
        context.uninstallApplication(paneId, bundle);
      return;
    }
    if (isCopySelectionAction(action.id)) {
      if (directory === undefined || directory.location === undefined) return;
      void copySelectionToClipboard(action.id, selectedEntries, directory.location)
        .then((copied) => {
          if (copied) context.getCommandPaletteRecency().set(action.id, Date.now());
          context.redraw();
        })
        .catch((error: unknown) => {
          context.toast({
            html: error instanceof Error ? error.message : t('clipboard', 'writeFailed'),
          });
          context.redraw();
        });
      return;
    }
    const effectiveParameters =
      parameters ?? platformActionParameters(action.id, selectedEntries, directory?.location);
    invokeActionById(action.id, effectiveParameters, contextParam);
  }

  function openContextMenu(
    paneId: PaneId,
    entries: readonly EntrySummary[],
    x: number,
    y: number,
  ): void {
    context.setContextMenu({ paneId, entries, x, y });
    context.redraw();
  }

  function invokeContextMenuAction(actionId: string): void {
    const menu = context.getContextMenu();
    if (menu === undefined) return;
    const action = context.getRegisteredActions().find((candidate) => candidate.id === actionId);
    const directory = context.getDirectories().get(context.getActiveTabKey(menu.paneId));
    if (action === undefined || directory === undefined) return;
    if (
      !evaluateActionAvailability(action, commandAvailabilityContext(menu.entries, menu.paneId))
        .available
    ) {
      return;
    }
    if (action.id === 'core.createDirectory') {
      context.openCreateDirectory(directory.location);
      return;
    }
    if (action.id === 'core.refresh') {
      void context.getNavigation().load(menu.paneId);
      return;
    }
    if (action.id === 'core.paste') {
      const currentClipboard = context.getClipboard();
      const mode = currentClipboard.mode;
      if (mode === undefined || directory.location === undefined) return;
      void (
        mode === 'move'
          ? context.getOpsController().move(currentClipboard.locations, directory.location)
          : context.getOpsController().copy(currentClipboard.locations, directory.location)
      ).then(() => {
        if (mode === 'move') context.replaceClipboard(clearClipboard(currentClipboard));
        context.redraw();
      });
      return;
    }
    if (action.id === 'core.pack' || action.id === 'core.moveToArchive') {
      if (menu.entries.length === 0 || directory.location === undefined) return;
      context.setArchiveCreateRequest({
        sources: menu.entries.map((entry) => entry.location),
        destinationDirectory: directory.location,
        moveSources: action.id === 'core.moveToArchive',
      });
      return;
    }
    if (action.id === 'core.extract') {
      const source = menu.entries[0];
      const workspace = context.getWorkspace();
      const otherPaneId = workspace?.paneOrder.find((paneId) => paneId !== menu.paneId);
      const destination =
        otherPaneId === undefined
          ? undefined
          : context.getDirectories().get(context.getActiveTabKey(otherPaneId))?.location;
      if (source === undefined || destination === undefined) return;
      void context.getOpsController().extract(source.location, destination);
      return;
    }
    if (openEntryMetadataDialog(action.id, menu.entries[0])) return;
    if (action.id === 'core.uninstallApplication') {
      const bundle = menu.entries[0];
      if (bundle !== undefined) context.uninstallApplication(menu.paneId, bundle);
      return;
    }
    invokePaletteAction(action, undefined, {
      paneId: menu.paneId,
      selectedEntryIds: menu.entries.map((entry) => entry.id),
      ...(menu.entries[0] === undefined ? {} : { cursorEntryId: menu.entries[0].id }),
    });
  }

  return {
    actionContext,
    commandAvailabilityContext,
    platformActionParameters,
    invokeActionById,
    invokePaletteAction,
    openContextMenu,
    invokeContextMenuAction,
  };
}
