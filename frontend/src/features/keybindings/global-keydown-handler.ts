import { toast } from 'mithril-materialized';
import { t } from '../../i18n';
import {
  dispatchKeybinding,
  hasPrimaryModifier,
  type KeybindingRuntime,
} from '../../keybindings/dispatcher';
import type {
  ActionDescriptor,
  ActionInvocationContext,
  ClipboardState,
  EntrySummary,
  Location,
  PaneId,
  Settings,
  SortDescriptor,
  WorkspaceProjection,
} from '../../models';
import { type AppState, applyAppPatches, setQuickFilterDraftPatch } from '../../state';
import {
  clearClipboard,
  copyToClipboard,
  cutToClipboard,
  validatePasteTarget,
} from '../clipboard/clipboard';
import type { CommandAvailabilityContext } from '../commands/availability';
import type { NavigationController, PaneDirectoryView } from '../navigation/navigation';
import { rootLocationFor } from '../navigation/root-location';
import type { OperationsController } from '../operations/operations-controller';
import { isParentEntry } from '../panes/parent-entry';
import type { TabController } from '../panes/tab-controller';
import type { FileViewerController, FileViewerState } from '../preview/file-viewer-controller';
import type { SelectionPlatform } from '../selection/keybindings';
import { getSelectedEntriesOrCursor, type SelectionState } from '../selection/selection';

/** Fixed sort applied by the Ctrl+F3..Ctrl+F7 shortcuts (Total Commander parity, task 0128) and
 * by the native macOS View menu's sort items (task 0133 follow-up) - exported so both dispatch
 * through this single mapping rather than maintaining two copies of it. */
export const SORT_SHORTCUT_DESCRIPTORS: Readonly<Record<string, readonly SortDescriptor[]>> = {
  'core.sortByName': [{ columnId: 'core.name', direction: 'ascending' }],
  'core.sortByExtension': [{ columnId: 'core.extension', direction: 'ascending' }],
  'core.sortByDate': [{ columnId: 'core.modified', direction: 'ascending' }],
  'core.sortBySize': [{ columnId: 'core.size', direction: 'ascending' }],
  'core.sortUnsorted': [],
};

type ArchiveCreateRequest = {
  readonly sources: readonly Location[];
  readonly destinationDirectory: Location;
  readonly moveSources: boolean;
};

type InitialSearch = {
  readonly query: string;
  readonly regex: boolean;
  readonly caseSensitive: boolean;
  readonly wholeWord: boolean;
};

export interface GlobalKeydownContext {
  // State getters
  getCommandPaletteOpen(): boolean;
  getPlatform(): SelectionPlatform;
  getKeybindingRuntime(): KeybindingRuntime;
  getCurrentSettings(): Settings | undefined;
  getWorkspace(): WorkspaceProjection | undefined;
  getSelections(): Map<string, SelectionState>;
  getDirectories(): Map<string, PaneDirectoryView>;
  getRegisteredActions(): readonly ActionDescriptor[];
  clipboard(): ClipboardState;
  getFindFilesOpen(): boolean;
  getViewer(
    paneId: PaneId,
  ): { readonly controller: FileViewerController; state: FileViewerState } | undefined;
  getArchiveCreateRequest(): ArchiveCreateRequest | undefined;
  getCreateDirectoryOpen(): boolean;
  getCreateFileOpen(): boolean;
  getAppState(): AppState | undefined;
  /** Last non-empty Quick Filter query committed on this pane's active tab (Ctrl+Shift+S). */
  getLastQuickFilterQuery(paneId: PaneId): string | undefined;
  getShortcutsHelpOpen(): boolean;

  // State setters
  setCommandPaletteOpen(open: boolean): void;
  setClipboardMessage(msg: string | undefined): void;
  setArchiveCreateRequest(req: ArchiveCreateRequest | undefined): void;
  setCreateDirectoryOpen(open: boolean): void;
  setCreateFileOpen(open: boolean): void;
  setAppState(state: AppState): void;
  setQuickFilterOpen(key: string, open: boolean): void;
  /** Sets (`query` defined) or clears (`query` undefined) the active tab's committed Quick Filter. */
  setActiveTabQuickFilter(paneId: PaneId, query: string | undefined): void;
  setConnectionsManagerOpen(open: boolean): void;
  setShortcutsHelpOpen(open: boolean): void;

  // Controller accessors
  getTabController(): TabController;
  getOpsController(): OperationsController;
  getNavigation(): NavigationController;

  // Helper functions
  activeDirectory(): { paneId: PaneId; location: Location } | undefined;
  activeTabKey(paneId: PaneId): string;
  actionsWithFavourites(): readonly ActionDescriptor[];
  openFindFiles(): void;
  replaceClipboard(next?: ClipboardState): void;
  selectedLocations(): readonly Location[];
  invokeActionById(actionId: string, parameters: unknown, ctx: ActionInvocationContext): void;
  openViewer(
    paneId: PaneId,
    entry: EntrySummary,
    initialSearch?: InitialSearch,
    openMetadata?: boolean,
  ): void;
  openEditor(paneId: PaneId, entry: EntrySummary): void;
  /** Recursively sums a directory's total size and fills in its Size cell once known (task 0071's
   * Total Commander-style folder-size key, Ctrl+.). */
  calculateFolderSize(paneId: PaneId, entry: EntrySummary): void;
  /** Scans a `.app` bundle's well-known related-file locations and opens the uninstall review
   * checklist (task 0148, macOS-only). */
  uninstallApplication(paneId: PaneId, entry: EntrySummary): void;
  actionContext(): ActionInvocationContext;
  commandAvailabilityContext(
    entries?: readonly EntrySummary[],
    paneId?: PaneId,
  ): CommandAvailabilityContext;
  contentSearchInitialQuery(locationUri: string, entry: EntrySummary): InitialSearch | undefined;
  refetchAffectedPanes(paneId?: PaneId): void;
  platformActionParameters(
    actionId: string,
    entries: readonly EntrySummary[],
    location: Location | undefined,
  ): { uri: string } | undefined;
  activatePane(paneId: PaneId): void;
  /** Moves DOM focus into `paneId`'s directory table, activating it as a side effect. Undefined
   * before the workspace layout has mounted and registered its focus callback. */
  focusPane(paneId: PaneId): void;
  /** Moves keyboard focus into `paneId`'s open F3 viewer: its find-in-file search input for text
   * content (Total Commander Lister's Tab-to-search convention), or the viewer's own focusable
   * section for content kinds without a search bar (image/pdf/...). No-op if no viewer is open. */
  focusViewer(paneId: PaneId): void;
  /** Moves keyboard focus to the open viewer's inline search input. */
  focusViewerSearch(paneId: PaneId): void;
  /** Scrolls (`unit: 'line'`, Arrow keys) or pages (`unit: 'page'`, Page Up/Down) `paneId`'s open
   * viewer's scrollable body by one step in the `(dx, dy)` direction - used both for vertical text
   * scrolling and (both-axis) image panning. No-op if no viewer is open. */
  scrollViewer(paneId: PaneId, dx: -1 | 0 | 1, dy: -1 | 0 | 1, unit: 'line' | 'page'): void;
  redraw(): void;
  toggleTerminal(): void;
  /** Toggles the directory-tree sidebar (Alt+F10, Total Commander parity, task 0139). */
  toggleDirectoryTree(): void;
  /** Toggles the operation centre (Alt+Z). */
  toggleOperationCentre(): void;
  /** Applies a fixed sort to `paneId`'s active tab (Ctrl+F3..Ctrl+F7). */
  setSort(paneId: PaneId, sort: readonly SortDescriptor[]): void;
  /** Swaps `paneAId` and `paneBId`'s entire tab sets (Ctrl+Shift+U), not just their active locations. */
  swapPaneTabSets(paneAId: PaneId, paneBId: PaneId): void;
  /** Opens the Multi-Rename Tool directly (Ctrl+M), defaulting to every entry when none is selected. */
  openMultiRenameForActivePane(): void;
  /** Opens the Properties dialog for the active pane's selection (Alt+Enter), falling back to the
   * cursor entry when nothing is explicitly selected. */
  openPropertiesForActivePane(): void;
  /** Closes the desktop window (Alt+F4); a no-op in browser runtime. */
  quitApplication(): void;
  /** Starts (or re-runs) a directory comparison of the first two panes (Shift+F2, task 0075).
   * Self-guards on fewer than two open panes, same as the toolbar's Compare button. */
  startComparison(): void;
  /** Starts a checksum job over the current selection (task 0077). */
  calculateChecksums(): void;
  /** Starts a duplicate scan rooted at the active pane's directory (task 0077). */
  findDuplicates(): void;
  /** Opens a disk-usage treemap for the active local directory in a new tab (task 0118). */
  openDiskUsage(): void;
  /** Opens the Settings dialog (Cmd+,/Ctrl+,) - a no-op if already open. */
  openSettingsDialog(): void;
}

function isEditableTarget(target: EventTarget | null): boolean {
  return (
    target instanceof HTMLInputElement ||
    target instanceof HTMLTextAreaElement ||
    target instanceof HTMLSelectElement ||
    (target instanceof HTMLElement && target.isContentEditable)
  );
}

function isWithinModal(target: EventTarget | null): boolean {
  return target instanceof HTMLElement && target.closest('[role="dialog"]') !== null;
}

/** Resolves *only* the cursor entry, ignoring any marked selection - F3/F4/Ctrl+Enter (view, edit,
 * open-with) are single-file commands that always act on whatever the cursor is on, Total
 * Commander style, unlike F5/F6 (copy/move) which prefer the marked set and fall back to the
 * cursor only when nothing is marked (`getSelectedEntriesOrCursor`). */
function cursorOnlyEntry(
  selection: SelectionState | undefined,
  entries: readonly EntrySummary[],
): readonly EntrySummary[] {
  const cursor =
    selection?.cursorEntryId === undefined
      ? undefined
      : entries.find((entry) => entry.id === selection.cursorEntryId);
  return cursor === undefined ? [] : [cursor];
}

/** Resolves the cursor entry F3 (or Alt+Space, when no viewer is already open) would open: the
 * single non-parent file entry under the active pane's cursor, and the opposite pane to open it
 * into. Shared so Alt+Space can open a viewer exactly the way F3 would, rather than duplicating
 * this resolution logic. */
function resolveViewTarget(context: GlobalKeydownContext):
  | {
      readonly paneId: PaneId;
      readonly entry: EntrySummary;
      readonly initialSearch?: InitialSearch;
    }
  | undefined {
  const workspace = context.getWorkspace();
  const active = context.activeDirectory();
  const selection =
    active === undefined
      ? undefined
      : context.getSelections().get(context.activeTabKey(active.paneId));
  const directory =
    active === undefined
      ? undefined
      : context.getDirectories().get(context.activeTabKey(active.paneId));
  const viewEntry =
    selection?.cursorEntryId === undefined
      ? undefined
      : directory?.entries.find((entry) => entry.id === selection.cursorEntryId);
  const otherPaneId = workspace?.paneOrder.find((paneId) => paneId !== active?.paneId);
  if (viewEntry === undefined || viewEntry.kind !== 'file' || isParentEntry(viewEntry.id))
    return undefined;
  if (otherPaneId === undefined) return undefined;
  const initialSearch =
    active === undefined
      ? undefined
      : context.contentSearchInitialQuery(active.location.uri, viewEntry);
  return {
    paneId: otherPaneId,
    entry: viewEntry,
    ...(initialSearch === undefined ? {} : { initialSearch }),
  };
}

/** Finds the currently open F3 viewer, regardless of which pane is active. There is only ever one
 * viewer open at a time app-wide (`openViewer` reuses/replaces the existing one rather than
 * allowing several), and F3 opens it in the *opposite* pane from the one the user pressed F3 in
 * without switching keyboard focus there - so gating on `getViewer(activePaneId)` alone made
 * next-match/Alt+Space/arrow-key paging only work while the source listing pane happened to be
 * the one showing the viewer, forcing a pane switch first. Checking every pane fixes that. */
function findOpenViewer(
  context: GlobalKeydownContext,
): ReturnType<GlobalKeydownContext['getViewer']> {
  const workspace = context.getWorkspace();
  if (workspace === undefined) return undefined;
  for (const paneId of workspace.paneOrder) {
    const viewer = context.getViewer(paneId);
    if (viewer !== undefined) return viewer;
  }
  return undefined;
}

/** Same lookup as `findOpenViewer`, but returns the pane it was found in - needed to target
 * `focusViewer`/`scrollViewer` (which act on a specific pane's DOM) rather than just the
 * viewer's own state/controller. */
function findOpenViewerPaneId(context: GlobalKeydownContext): PaneId | undefined {
  const workspace = context.getWorkspace();
  if (workspace === undefined) return undefined;
  for (const paneId of workspace.paneOrder) {
    if (context.getViewer(paneId) !== undefined) return paneId;
  }
  return undefined;
}

/** Whether `target` is inside an open F3 viewer's DOM subtree (`.fm-pane-viewer`, set on the
 * pane's own section - see `pane.ts`). Gates viewer-scoped Arrow/Page/zoom keys so they only take
 * over once focus has actually moved into the viewer (e.g. via Tab - see the `core.switchPane`
 * override below), rather than firing from anywhere in the app just because a viewer happens to be
 * open somewhere - unlike `findOpenViewer`'s PDF/comic/EPUB paging, ArrowUp/Down are already bound
 * to move the cursor in a focused directory table, so this must not compete with that. */
function isWithinViewer(target: EventTarget | null): boolean {
  return target instanceof HTMLElement && target.closest('.fm-pane-viewer') !== null;
}

/** Whether Arrow/Page/zoom keys should drive an open F3 viewer's content from `target`: true
 * anywhere inside the viewer (`isWithinViewer`) *except* where that would steal real text editing
 * - e.g. Left/Right moving the text cursor while renaming a tab in the viewer's own `TabStrip`.
 * The viewer's find-in-file search input and the (read-only, but DOM-`contenteditable` for text
 * selection) CodeMirror body are allowed through despite `isEditableTarget` flagging them: Arrow
 * Up/Down/Page keys have no text-editing meaning in a single-line search box, and the CodeMirror
 * body is exactly where focus lands while reading the file - blocking navigation there would
 * defeat the point of Tab moving focus into the viewer in the first place. */
function isViewerNavigationTarget(target: EventTarget | null): boolean {
  if (!isWithinViewer(target)) return false;
  if (!isEditableTarget(target)) return true;
  return (
    target instanceof HTMLElement &&
    (target.matches('.fm-file-viewer-search-input') || target.closest('.cm-editor') !== null)
  );
}

function canUseSystemTrash(locations: readonly Location[]): boolean {
  return locations.every(
    (location) => location.providerId === 'file' || location.providerId === 'local',
  );
}

/** Total Commander's directory-tree shortcut (task 0139). Alt+F10 alone, since Ctrl+F10 is
 * already `core.clearQuickFilter`. */
export function isDirectoryTreeToggleShortcut(
  event: Pick<KeyboardEvent, 'key' | 'code' | 'altKey' | 'ctrlKey' | 'metaKey' | 'shiftKey'>,
): boolean {
  const bareModifiers = !event.ctrlKey && !event.metaKey && !event.shiftKey;
  return bareModifiers && event.altKey && (event.key === 'F10' || event.code === 'F10');
}

export function isOperationCentreToggleShortcut(
  event: Pick<KeyboardEvent, 'key' | 'code' | 'altKey' | 'ctrlKey' | 'metaKey' | 'shiftKey'>,
): boolean {
  return (
    event.altKey &&
    !event.ctrlKey &&
    !event.metaKey &&
    !event.shiftKey &&
    (event.key.toLowerCase() === 'z' || event.code === 'KeyZ')
  );
}

type KeydownRouteState = {
  dispatchedAction: string | undefined;
  forceSystemView: boolean;
  forceSystemEdit: boolean;
};

type KeydownRoute = {
  readonly id: string;
  readonly tryHandle: (
    context: GlobalKeydownContext,
    event: KeyboardEvent,
    state: KeydownRouteState,
  ) => boolean | undefined;
};

const EARLY_KEYDOWN_ROUTES = [
  {
    id: 'modal-blocker',
    tryHandle: (_context, event) => {
      if (isWithinModal(event.target)) return;
      return false;
    },
  },
  {
    id: 'terminal-toggle',
    tryHandle: (context, event) => {
      if (!isTerminalToggleShortcut(event, context.getKeybindingRuntime())) return false;
      event.preventDefault();
      context.toggleTerminal();
      context.redraw();
      return true;
    },
  },
  {
    id: 'operation-centre-toggle',
    tryHandle: (context, event) => {
      if (!isOperationCentreToggleShortcut(event)) return false;
      event.preventDefault();
      context.toggleOperationCentre();
      context.redraw();
      return true;
    },
  },
  {
    id: 'directory-tree-toggle',
    tryHandle: (context, event) => {
      if (!isDirectoryTreeToggleShortcut(event)) return false;
      event.preventDefault();
      context.toggleDirectoryTree();
      context.redraw();
      return true;
    },
  },
  {
    id: 'command-palette-blocker',
    tryHandle: (context) => {
      if (context.getCommandPaletteOpen()) return;
      return false;
    },
  },
  {
    id: 'command-palette-open',
    tryHandle: (context, event) => {
      if (
        !hasPrimaryModifier(event, context.getPlatform()) ||
        event.altKey ||
        event.key.toLowerCase() !== 'p'
      )
        return false;
      event.preventDefault();
      context.setCommandPaletteOpen(true);
      context.redraw();
      return true;
    },
  },
  {
    id: 'settings-open',
    tryHandle: (context, event) => {
      // Cmd+, (Ctrl+, elsewhere) opens Settings - the standard desktop-app "Preferences" shortcut,
      // same treatment as Ctrl+P above (a pure UI toggle with no backend action, so it's
      // special-cased here rather than routed through the action registry/`dispatchKeybinding`).
      if (!hasPrimaryModifier(event, context.getPlatform()) || event.altKey || event.key !== ',')
        return false;
      event.preventDefault();
      context.openSettingsDialog();
      context.redraw();
      return true;
    },
  },
] as const satisfies readonly KeydownRoute[];

const ACTION_KEYDOWN_ROUTES = [
  // ArrowLeft/ArrowRight page through an open PDF/comic/EPUB/PPTX viewer - Total Commander's Lister
  // convention for paged content. Works regardless of which pane is active (see
  // `findOpenViewer`): F3 opens the viewer in the *opposite* pane without moving keyboard focus
  // there, so requiring the viewer's own pane to be active would force a pane switch first just
  // to page through it. Unbound elsewhere in the directory table (only ArrowUp/Down move the
  // cursor there), so no conflict/stopPropagation is needed.
  {
    id: 'viewer-paged',
    tryHandle: (context, event) => {
      if (
        !isEditableTarget(event.target) &&
        !event.altKey &&
        !event.ctrlKey &&
        !event.metaKey &&
        !event.shiftKey &&
        (event.key === 'ArrowLeft' || event.key === 'ArrowRight')
      ) {
        const activeViewer = findOpenViewer(context);
        const content =
          activeViewer !== undefined && activeViewer.state.status === 'ready'
            ? activeViewer.state.content
            : undefined;
        if (
          content !== undefined &&
          (content.kind === 'pdf' || content.kind === 'comic' || content.kind === 'epub')
        ) {
          event.preventDefault();
          if (event.key === 'ArrowLeft') activeViewer?.controller.previousPage();
          else activeViewer?.controller.nextPage();
          context.redraw();
          return;
        }
      }
      return false;
    },
  },
  // Arrow/Page keys inside an open F3 viewer: scroll text, page-scroll DOCX, page PDFs, or pan/zoom images.
  // Gated on `isViewerNavigationTarget` - see its doc comment for why this must not fire from
  // just anywhere the way ArrowLeft/Right paging above does.
  {
    id: 'viewer-navigation',
    tryHandle: (context, event) => {
      if (
        !event.altKey &&
        !event.ctrlKey &&
        !event.metaKey &&
        !event.shiftKey &&
        isViewerNavigationTarget(event.target) &&
        (event.key === 'ArrowUp' ||
          event.key === 'ArrowDown' ||
          event.key === 'ArrowLeft' ||
          event.key === 'ArrowRight' ||
          event.key === 'PageUp' ||
          event.key === 'PageDown')
      ) {
        const viewerPaneId = findOpenViewerPaneId(context);
        const activeViewer =
          viewerPaneId === undefined ? undefined : context.getViewer(viewerPaneId);
        const content =
          activeViewer !== undefined && activeViewer.state.status === 'ready'
            ? activeViewer.state.content
            : undefined;
        if (
          viewerPaneId !== undefined &&
          (content?.kind === 'text' || content?.kind === 'docx' || content?.kind === 'epub')
        ) {
          if (content.kind === 'text' && (event.key === 'ArrowUp' || event.key === 'ArrowDown')) {
            event.preventDefault();
            context.scrollViewer(viewerPaneId, 0, event.key === 'ArrowUp' ? -1 : 1, 'line');
            return;
          }
          if (event.key === 'PageUp' || event.key === 'PageDown') {
            event.preventDefault();
            context.scrollViewer(viewerPaneId, 0, event.key === 'PageUp' ? -1 : 1, 'page');
            return;
          }
        }
        if (
          viewerPaneId !== undefined &&
          content?.kind === 'pdf' &&
          (event.key === 'PageUp' || event.key === 'PageDown')
        ) {
          event.preventDefault();
          if (event.key === 'PageUp') activeViewer?.controller.previousPage();
          else activeViewer?.controller.nextPage();
          context.redraw();
          return;
        }
        if (viewerPaneId !== undefined && content?.kind === 'image') {
          if (event.key === 'PageUp' || event.key === 'PageDown') {
            event.preventDefault();
            if (event.key === 'PageUp') activeViewer?.controller.zoomIn();
            else activeViewer?.controller.zoomOut();
            context.redraw();
            return;
          }
          const dx = event.key === 'ArrowLeft' ? -1 : event.key === 'ArrowRight' ? 1 : 0;
          const dy = event.key === 'ArrowUp' ? -1 : event.key === 'ArrowDown' ? 1 : 0;
          if (dx !== 0 || dy !== 0) {
            event.preventDefault();
            context.scrollViewer(viewerPaneId, dx, dy, 'line');
            return;
          }
        }
      }
      return false;
    },
  },
  // Mod+F focuses the inline search field consistently across searchable viewer implementations.
  {
    id: 'viewer-search-focus',
    tryHandle: (context, event) => {
      if (
        hasPrimaryModifier(event, context.getPlatform()) &&
        !event.altKey &&
        event.key.toLowerCase() === 'f' &&
        isWithinViewer(event.target)
      ) {
        const viewerPaneId = findOpenViewerPaneId(context);
        const activeViewer =
          viewerPaneId === undefined ? undefined : context.getViewer(viewerPaneId);
        const content =
          activeViewer !== undefined && activeViewer.state.status === 'ready'
            ? activeViewer.state.content
            : undefined;
        if (
          viewerPaneId !== undefined &&
          (content?.kind === 'text' ||
            content?.kind === 'docx' ||
            content?.kind === 'pdf' ||
            content?.kind === 'epub')
        ) {
          event.preventDefault();
          context.focusViewerSearch(viewerPaneId);
          return;
        }
      }
      return false;
    },
  },
  // Mod+/- zoom an open F3 viewer's scalable content while preserving the image viewer's existing
  // bare +/- shortcuts. '+' itself requires Shift on most keyboard layouts, so this intentionally
  // does not reject `shiftKey`.
  {
    id: 'viewer-zoom',
    tryHandle: (context, event) => {
      if (
        !event.altKey &&
        ((!event.ctrlKey && !event.metaKey) || hasPrimaryModifier(event, context.getPlatform())) &&
        isViewerNavigationTarget(event.target) &&
        (event.key === '+' || event.key === '=' || event.key === '-')
      ) {
        const viewerPaneId = findOpenViewerPaneId(context);
        const activeViewer =
          viewerPaneId === undefined ? undefined : context.getViewer(viewerPaneId);
        const content =
          activeViewer !== undefined && activeViewer.state.status === 'ready'
            ? activeViewer.state.content
            : undefined;
        if (content?.kind === 'image' || content?.kind === 'epub') {
          event.preventDefault();
          if (event.key === '-') activeViewer?.controller.zoomOut();
          else activeViewer?.controller.zoomIn();
          context.redraw();
          return;
        }
      }
      return false;
    },
  },
  // F3/Shift+F3 navigate search matches once focus is inside a searchable F3 viewer
  // content - the standard browser/Lister find-next/previous convention, and the one reliable
  // way to do this once Tab has moved focus into the viewer (see `isViewerNavigationTarget`):
  // `core.view`'s own "F3 repeats as next match" below only fires when `activePaneId` itself is
  // the viewer's pane, which Tab-into-search deliberately never changes.
  {
    id: 'viewer-search',
    tryHandle: (context, event) => {
      if (
        !event.ctrlKey &&
        !event.metaKey &&
        !event.altKey &&
        event.key === 'F3' &&
        isViewerNavigationTarget(event.target)
      ) {
        const viewerPaneId = findOpenViewerPaneId(context);
        const activeViewer =
          viewerPaneId === undefined ? undefined : context.getViewer(viewerPaneId);
        const content =
          activeViewer !== undefined && activeViewer.state.status === 'ready'
            ? activeViewer.state.content
            : undefined;
        if (content?.kind === 'text' || content?.kind === 'docx') {
          event.preventDefault();
          if (event.shiftKey) activeViewer?.controller.goToPreviousMatch();
          else activeViewer?.controller.goToNextMatch();
          context.redraw();
          return;
        }
        if (content?.kind === 'pdf') {
          event.preventDefault();
          if (event.shiftKey) activeViewer?.controller.goToPreviousPdfMatch();
          else activeViewer?.controller.goToNextPdfMatch();
          context.redraw();
          return;
        }
        if (content?.kind === 'epub') {
          event.preventDefault();
          if (event.shiftKey) activeViewer?.controller.goToPreviousEpubMatch();
          else activeViewer?.controller.goToNextEpubMatch();
          context.redraw();
          return;
        }
      }
      return false;
    },
  },
  // Alt+Space calculates the cursored directory's size. For files it shows the metadata/info
  // panel: if a viewer is already open in the active pane, it
  // toggles that viewer's panel; otherwise it opens a viewer for the cursor entry (exactly like
  // F3) with the panel shown immediately - so the shortcut works from the directory listing too,
  // not only once a viewer happens to be open. Plain Space is left alone - it already toggles
  // (and, per Total Commander, advances past) the cursor row's selection in the pane
  // (`selection/keybindings.ts`), and must not collide with this.
  {
    id: 'alt-space',
    tryHandle: (context, event) => {
      if (!isEditableTarget(event.target) && event.altKey && event.code === 'Space') {
        const active = context.activeDirectory();
        const selection =
          active === undefined
            ? undefined
            : context.getSelections().get(context.activeTabKey(active.paneId));
        const directory =
          active === undefined
            ? undefined
            : context.getDirectories().get(context.activeTabKey(active.paneId));
        const cursorEntry =
          selection?.cursorEntryId === undefined
            ? undefined
            : directory?.entries.find((entry) => entry.id === selection.cursorEntryId);
        if (
          active !== undefined &&
          cursorEntry?.kind === 'directory' &&
          !isParentEntry(cursorEntry.id)
        ) {
          event.preventDefault();
          context.calculateFolderSize(active.paneId, cursorEntry);
          return;
        }
        const activeViewer = findOpenViewer(context);
        if (activeViewer !== undefined) {
          event.preventDefault();
          activeViewer.controller.toggleMetadataPanel();
          context.redraw();
          return;
        }
        const target = resolveViewTarget(context);
        if (target !== undefined) {
          event.preventDefault();
          context.openViewer(target.paneId, target.entry, target.initialSearch, true);
          return;
        }
      }
      return false;
    },
  },
  {
    id: 'core.favourites',
    tryHandle: (context, event, state) => {
      if (state.dispatchedAction === 'core.favourites') {
        event.preventDefault();
        context.setCommandPaletteOpen(true);
        context.redraw();
        return;
      }
      return false;
    },
  },
  {
    id: 'core.favourite-index',
    tryHandle: (context, event, state) => {
      if (state.dispatchedAction?.startsWith('core.favourite.')) {
        const index = Number(state.dispatchedAction.slice('core.favourite.'.length));
        const favourite = context.getCurrentSettings()?.favouriteLocations[index];
        const active = context.activeDirectory();
        if (favourite !== undefined && active !== undefined) {
          event.preventDefault();
          void context.getNavigation().navigate(active.paneId, favourite.location);
        }
        return;
      }
      return false;
    },
  },
  {
    id: 'primary-modifier',
    tryHandle: (context, event) => {
      if (
        !isEditableTarget(event.target) &&
        hasPrimaryModifier(event, context.getPlatform()) &&
        !event.altKey
      ) {
        const key = event.key.toLowerCase();
        const sources = context.selectedLocations();
        if ((key === 'c' || key === 'x') && sources.length > 0) {
          event.preventDefault();
          context.replaceClipboard(
            key === 'c'
              ? copyToClipboard(context.clipboard(), sources)
              : cutToClipboard(context.clipboard(), sources),
          );
          context.setClipboardMessage(undefined);
          context.redraw();
          return;
        }
        if (key === 'v') {
          event.preventDefault();
          const active = context.activeDirectory();
          const directory =
            active === undefined
              ? undefined
              : context.getDirectories().get(context.activeTabKey(active.paneId));
          const currentClipboard = context.clipboard();
          const target =
            active === undefined || directory === undefined
              ? undefined
              : {
                  location: active.location,
                  writable: directory.writable === true,
                  loaded: directory.state.type === 'loaded',
                };
          const validation = validatePasteTarget(currentClipboard, target);
          if (!validation.ok) {
            context.setClipboardMessage(validation.message);
            context.redraw();
            return;
          }
          const mode = currentClipboard.mode;
          if (mode === undefined || active === undefined) return;
          context.setClipboardMessage(undefined);
          void (
            mode === 'move'
              ? context.getOpsController().move(currentClipboard.locations, active.location)
              : context.getOpsController().copy(currentClipboard.locations, active.location)
          )
            .then(() => {
              if (mode === 'move') context.replaceClipboard(clearClipboard(currentClipboard));
              context.redraw();
            })
            .catch((error: unknown) => {
              context.setClipboardMessage(
                error instanceof Error ? error.message : t('clipboard', 'pasteFailed'),
              );
              context.redraw();
            });
          return;
        }
        if (key >= '1' && key <= '9') {
          const active = context.activeDirectory();
          if (active !== undefined) {
            event.preventDefault();
            context.getTabController().jumpToTab(active.paneId, Number(key));
          }
          return;
        }
        return false;
      }
      return false;
    },
  },
  {
    id: 'core.copy',
    tryHandle: (context, event, state) => {
      if (state.dispatchedAction === 'core.copy') {
        const active = context.activeDirectory();
        const selection =
          active === undefined
            ? undefined
            : context.getSelections().get(context.activeTabKey(active.paneId));
        const directory =
          active === undefined
            ? undefined
            : context.getDirectories().get(context.activeTabKey(active.paneId));
        const selected = getSelectedEntriesOrCursor(selection, directory?.entries ?? []);
        const workspace = context.getWorkspace();
        const otherPaneId = workspace?.paneOrder.find((paneId) => paneId !== active?.paneId);
        const destination =
          otherPaneId === undefined
            ? undefined
            : context.getDirectories().get(context.activeTabKey(otherPaneId))?.location;
        if (selected.length > 0 && destination !== undefined) {
          event.preventDefault();
          void context.getOpsController().copy(
            selected.map((entry) => entry.location),
            destination,
          );
        }
        return;
      }
      return false;
    },
  },
  {
    id: 'core.pack',
    tryHandle: (context, event, state) => {
      if (state.dispatchedAction === 'core.pack') {
        const active = context.activeDirectory();
        const selection =
          active === undefined
            ? undefined
            : context.getSelections().get(context.activeTabKey(active.paneId));
        const directory =
          active === undefined
            ? undefined
            : context.getDirectories().get(context.activeTabKey(active.paneId));
        const selected = getSelectedEntriesOrCursor(selection, directory?.entries ?? []);
        if (selected.length > 0 && directory?.location !== undefined) {
          event.preventDefault();
          context.setArchiveCreateRequest({
            sources: selected.map((entry) => entry.location),
            destinationDirectory: directory.location,
            moveSources: false,
          });
        }
        return;
      }
      return false;
    },
  },
  {
    id: 'core.moveToArchive',
    tryHandle: (context, event, state) => {
      if (state.dispatchedAction === 'core.moveToArchive') {
        const active = context.activeDirectory();
        const selection =
          active === undefined
            ? undefined
            : context.getSelections().get(context.activeTabKey(active.paneId));
        const directory =
          active === undefined
            ? undefined
            : context.getDirectories().get(context.activeTabKey(active.paneId));
        const selected = getSelectedEntriesOrCursor(selection, directory?.entries ?? []);
        if (selected.length > 0 && directory?.location !== undefined) {
          event.preventDefault();
          context.setArchiveCreateRequest({
            sources: selected.map((entry) => entry.location),
            destinationDirectory: directory.location,
            moveSources: true,
          });
        }
        return;
      }
      return false;
    },
  },
  {
    id: 'core.extract',
    tryHandle: (context, event, state) => {
      if (state.dispatchedAction === 'core.extract') {
        const active = context.activeDirectory();
        const selection =
          active === undefined
            ? undefined
            : context.getSelections().get(context.activeTabKey(active.paneId));
        const directory =
          active === undefined
            ? undefined
            : context.getDirectories().get(context.activeTabKey(active.paneId));
        const cursor = selection?.cursorEntryId;
        const selected = directory?.entries.filter((entry) => entry.id === cursor);
        const workspace = context.getWorkspace();
        const otherPaneId = workspace?.paneOrder.find((paneId) => paneId !== active?.paneId);
        const destination =
          otherPaneId === undefined
            ? undefined
            : context.getDirectories().get(context.activeTabKey(otherPaneId))?.location;
        const selectedEntry = selected?.length === 1 ? selected[0] : undefined;
        if (selectedEntry !== undefined && destination !== undefined) {
          event.preventDefault();
          void context.getOpsController().extract(selectedEntry.location, destination);
        }
        return;
      }
      return false;
    },
  },
  {
    id: 'core.move',
    tryHandle: (context, event, state) => {
      if (state.dispatchedAction === 'core.move') {
        const active = context.activeDirectory();
        const selection =
          active === undefined
            ? undefined
            : context.getSelections().get(context.activeTabKey(active.paneId));
        const directory =
          active === undefined
            ? undefined
            : context.getDirectories().get(context.activeTabKey(active.paneId));
        const selected = getSelectedEntriesOrCursor(selection, directory?.entries ?? []);
        const workspace = context.getWorkspace();
        const otherPaneId = workspace?.paneOrder.find((paneId) => paneId !== active?.paneId);
        const destination =
          otherPaneId === undefined
            ? undefined
            : context.getDirectories().get(context.activeTabKey(otherPaneId))?.location;
        if (selected.length > 0 && destination !== undefined) {
          event.preventDefault();
          void context.getOpsController().move(
            selected.map((entry) => entry.location),
            destination,
          );
        }
        return;
      }
      return false;
    },
  },
  {
    id: 'core.trash',
    tryHandle: (context, event, state) => {
      if (state.dispatchedAction === 'core.trash') {
        const active = context.activeDirectory();
        const selection =
          active === undefined
            ? undefined
            : context.getSelections().get(context.activeTabKey(active.paneId));
        const directory =
          active === undefined
            ? undefined
            : context.getDirectories().get(context.activeTabKey(active.paneId));
        const selected = getSelectedEntriesOrCursor(selection, directory?.entries ?? []);
        if (selected.length > 0) {
          event.preventDefault();
          const locations = selected.map((entry) => entry.location);
          if (canUseSystemTrash(locations)) {
            void context.getOpsController().trash(locations);
          } else {
            void context
              .getOpsController()
              .delete(
                locations,
                context.getCurrentSettings()?.confirmPermanentDelete === false,
                false,
              );
          }
        }
        return;
      }
      return false;
    },
  },
  {
    id: 'core.delete',
    tryHandle: (context, event, state) => {
      if (state.dispatchedAction === 'core.delete') {
        const active = context.activeDirectory();
        const selection =
          active === undefined
            ? undefined
            : context.getSelections().get(context.activeTabKey(active.paneId));
        const directory =
          active === undefined
            ? undefined
            : context.getDirectories().get(context.activeTabKey(active.paneId));
        const selected = getSelectedEntriesOrCursor(selection, directory?.entries ?? []);
        if (selected.length > 0) {
          event.preventDefault();
          void context.getOpsController().delete(
            selected.map((entry) => entry.location),
            context.getCurrentSettings()?.confirmPermanentDelete === false,
            false,
          );
        }
        return;
      }
      return false;
    },
  },
  {
    id: 'core.createDirectory',
    tryHandle: (context, event, state) => {
      if (
        state.dispatchedAction === 'core.createDirectory' &&
        !context.getCreateDirectoryOpen() &&
        context.activeDirectory() !== undefined
      ) {
        event.preventDefault();
        context.setCreateDirectoryOpen(true);
        context.redraw();
        return;
      }
      return false;
    },
  },
  {
    id: 'core.findFiles',
    tryHandle: (context, event, state) => {
      if (state.dispatchedAction === 'core.findFiles' && !context.getFindFilesOpen()) {
        const active = context.activeDirectory();
        if (active === undefined) return;
        event.preventDefault();
        context.openFindFiles();
        context.redraw();
        return;
      }
      return false;
    },
  },
  {
    id: 'core.quickFilter',
    tryHandle: (context, event, state) => {
      if (state.dispatchedAction === 'core.quickFilter') {
        const active = context.activeDirectory();
        if (active === undefined) return;
        event.preventDefault();
        const key = context.activeTabKey(active.paneId);
        context.setQuickFilterOpen(key, true);
        const appState = context.getAppState();
        if (appState === undefined) return;
        if (!(key in (appState?.quickFilterDrafts.byTabKey ?? {}))) {
          const workspace = context.getWorkspace();
          const pane = workspace?.panesById[active.paneId];
          const tab = pane?.tabsById[pane.activeTabId];
          context.setAppState(
            applyAppPatches(
              appState,
              setQuickFilterDraftPatch(key, tab?.view.quickFilter?.query ?? ''),
            ),
          );
        }
        context.redraw();
        return;
      }
      return false;
    },
  },
  {
    id: 'core.newTab',
    tryHandle: (context, event, state) => {
      if (state.dispatchedAction === 'core.newTab') {
        const active = context.activeDirectory();
        if (active === undefined) return;
        event.preventDefault();
        context.getTabController().openTab(active.paneId);
        return;
      }
      return false;
    },
  },
  {
    id: 'core.switchPane',
    tryHandle: (context, event, state) => {
      if (state.dispatchedAction === 'core.switchPane') {
        const workspace = context.getWorkspace();
        if (workspace === undefined) return;
        event.preventDefault();
        const paneOrder = workspace.paneOrder;
        if (paneOrder.length < 2) return;
        const currentIndex = paneOrder.indexOf(workspace.activePaneId);
        if (currentIndex < 0) return;
        const direction = event.shiftKey ? -1 : 1;
        const nextIndex = (currentIndex + direction + paneOrder.length) % paneOrder.length;
        const nextPaneId = paneOrder[nextIndex];
        // Move DOM focus into the target pane, not just app-state `activePaneId` - otherwise
        // arrow-key navigation stays inert until a mouse click focuses a row.
        if (nextPaneId !== undefined) context.focusPane(nextPaneId);
        return;
      }
      return false;
    },
  },
  {
    id: 'core.closeTab',
    tryHandle: (context, event, state) => {
      if (state.dispatchedAction === 'core.closeTab') {
        const workspace = context.getWorkspace();
        if (workspace === undefined) return;
        const paneId = workspace.activePaneId;
        const pane = workspace.panesById[paneId];
        if (pane === undefined) return;
        event.preventDefault();
        context.getTabController().requestCloseTab(paneId, pane.activeTabId);
        return;
      }
      return false;
    },
  },
  {
    id: 'core.tab-cycle',
    tryHandle: (context, event, state) => {
      if (
        state.dispatchedAction === 'core.nextTab' ||
        state.dispatchedAction === 'core.previousTab'
      ) {
        const workspace = context.getWorkspace();
        if (workspace === undefined) return;
        event.preventDefault();
        context
          .getTabController()
          .cycleTab(workspace.activePaneId, state.dispatchedAction === 'core.nextTab' ? 1 : -1);
        return;
      }
      return false;
    },
  },
  {
    id: 'core.reopenClosedTab',
    tryHandle: (context, event, state) => {
      if (state.dispatchedAction === 'core.reopenClosedTab') {
        const workspace = context.getWorkspace();
        if (workspace === undefined) return;
        event.preventDefault();
        context.getTabController().reopenClosedTab(workspace.activePaneId);
        return;
      }
      return false;
    },
  },
  {
    id: 'core.rootDirectory',
    tryHandle: (context, event, state) => {
      if (state.dispatchedAction === 'core.rootDirectory') {
        const active = context.activeDirectory();
        if (active === undefined) return;
        event.preventDefault();
        void context.getNavigation().navigate(active.paneId, rootLocationFor(active.location));
        return;
      }
      return false;
    },
  },
  {
    id: 'core.openInNewTab',
    tryHandle: (context, event, state) => {
      if (
        state.dispatchedAction === 'core.openInNewTab' ||
        state.dispatchedAction === 'core.openInNewTabOtherPane'
      ) {
        const active = context.activeDirectory();
        if (active === undefined) return;
        const key = context.activeTabKey(active.paneId);
        const selection = context.getSelections().get(key);
        const directory = context.getDirectories().get(key);
        const cursorEntry = directory?.entries.find(
          (entry) => entry.id === selection?.cursorEntryId,
        );
        if (
          cursorEntry === undefined ||
          cursorEntry.kind !== 'directory' ||
          isParentEntry(cursorEntry.id)
        )
          return;
        const workspace = context.getWorkspace();
        const targetPaneId =
          state.dispatchedAction === 'core.openInNewTabOtherPane'
            ? workspace?.paneOrder.find((paneId) => paneId !== active.paneId)
            : active.paneId;
        if (targetPaneId === undefined) return;
        event.preventDefault();
        context.getTabController().openTabAt(targetPaneId, cursorEntry.location);
        return;
      }
      return false;
    },
  },
  {
    id: 'core.duplicateLocationToOtherPane',
    tryHandle: (context, event, state) => {
      if (state.dispatchedAction === 'core.duplicateLocationToOtherPane') {
        const active = context.activeDirectory();
        if (active === undefined) return;
        const workspace = context.getWorkspace();
        const otherPaneId = workspace?.paneOrder.find((paneId) => paneId !== active.paneId);
        if (otherPaneId === undefined) return;
        event.preventDefault();
        void context.getNavigation().navigate(otherPaneId, active.location);
        return;
      }
      return false;
    },
  },
  {
    id: 'core.compareDirectories',
    tryHandle: (context, event, state) => {
      if (state.dispatchedAction === 'core.compareDirectories') {
        event.preventDefault();
        context.startComparison();
        return;
      }
      return false;
    },
  },
  {
    id: 'core.calculateChecksum',
    tryHandle: (context, event, state) => {
      if (state.dispatchedAction === 'core.calculateChecksum') {
        event.preventDefault();
        context.calculateChecksums();
        return;
      }
      return false;
    },
  },
  {
    id: 'core.findDuplicates',
    tryHandle: (context, event, state) => {
      if (state.dispatchedAction === 'core.findDuplicates') {
        event.preventDefault();
        context.findDuplicates();
        return;
      }
      return false;
    },
  },
  {
    id: 'client.diskUsage',
    tryHandle: (context, event, state) => {
      if (state.dispatchedAction === 'client.diskUsage') {
        event.preventDefault();
        context.openDiskUsage();
        return;
      }
      return false;
    },
  },
  {
    id: 'core.swapPanes',
    tryHandle: (context, event, state) => {
      if (state.dispatchedAction === 'core.swapPanes') {
        const workspace = context.getWorkspace();
        if (workspace === undefined || workspace.paneOrder.length < 2) return;
        const [paneAId, paneBId] = workspace.paneOrder;
        if (paneAId === undefined || paneBId === undefined) return;
        const locationA = context.getDirectories().get(context.activeTabKey(paneAId))?.location;
        const locationB = context.getDirectories().get(context.activeTabKey(paneBId))?.location;
        if (locationA === undefined || locationB === undefined) return;
        event.preventDefault();
        void context.getNavigation().navigate(paneAId, locationB);
        void context.getNavigation().navigate(paneBId, locationA);
        return;
      }
      return false;
    },
  },
  {
    id: 'core.swapPaneTabs',
    tryHandle: (context, event, state) => {
      if (state.dispatchedAction === 'core.swapPaneTabs') {
        const workspace = context.getWorkspace();
        if (workspace === undefined || workspace.paneOrder.length < 2) return;
        const [paneAId, paneBId] = workspace.paneOrder;
        if (paneAId === undefined || paneBId === undefined) return;
        event.preventDefault();
        context.swapPaneTabSets(paneAId, paneBId);
        return;
      }
      return false;
    },
  },
  {
    id: 'core.closeAllTabs',
    tryHandle: (context, event, state) => {
      if (state.dispatchedAction === 'core.closeAllTabs') {
        const workspace = context.getWorkspace();
        if (workspace === undefined) return;
        event.preventDefault();
        context.getTabController().closeAllTabs(workspace.activePaneId);
        return;
      }
      return false;
    },
  },
  {
    id: 'core.newConnection',
    tryHandle: (context, event, state) => {
      if (state.dispatchedAction === 'core.newConnection') {
        event.preventDefault();
        context.setConnectionsManagerOpen(true);
        context.redraw();
        return;
      }
      return false;
    },
  },
  {
    id: 'core.reactivateQuickFilter',
    tryHandle: (context, event, state) => {
      if (state.dispatchedAction === 'core.reactivateQuickFilter') {
        const active = context.activeDirectory();
        if (active === undefined) return;
        const last = context.getLastQuickFilterQuery(active.paneId);
        if (last === undefined || last.length === 0) return;
        event.preventDefault();
        context.setActiveTabQuickFilter(active.paneId, last);
        context.redraw();
        return;
      }
      return false;
    },
  },
  {
    id: 'core.clearQuickFilter',
    tryHandle: (context, event, state) => {
      if (state.dispatchedAction === 'core.clearQuickFilter') {
        const active = context.activeDirectory();
        if (active === undefined) return;
        event.preventDefault();
        context.setActiveTabQuickFilter(active.paneId, undefined);
        context.redraw();
        return;
      }
      return false;
    },
  },
  {
    id: 'core.sort',
    tryHandle: (context, event, state) => {
      if (
        state.dispatchedAction !== undefined &&
        state.dispatchedAction in SORT_SHORTCUT_DESCRIPTORS
      ) {
        const active = context.activeDirectory();
        if (active === undefined) return;
        event.preventDefault();
        context.setSort(active.paneId, SORT_SHORTCUT_DESCRIPTORS[state.dispatchedAction] ?? []);
        return;
      }
      return false;
    },
  },
  {
    id: 'core.createFile',
    tryHandle: (context, event, state) => {
      if (
        state.dispatchedAction === 'core.createFile' &&
        !context.getCreateFileOpen() &&
        context.activeDirectory() !== undefined
      ) {
        event.preventDefault();
        context.setCreateFileOpen(true);
        context.redraw();
        return;
      }
      return false;
    },
  },
  {
    id: 'core.duplicate',
    tryHandle: (context, event, state) => {
      if (state.dispatchedAction === 'core.duplicate') {
        const sources = context.selectedLocations();
        if (sources.length === 0) return;
        event.preventDefault();
        void context
          .getOpsController()
          .duplicate(sources)
          .then(() => context.refetchAffectedPanes());
        return;
      }
      return false;
    },
  },
  {
    id: 'core.openMultiRename',
    tryHandle: (context, event, state) => {
      if (state.dispatchedAction === 'core.openMultiRename') {
        event.preventDefault();
        context.openMultiRenameForActivePane();
        return;
      }
      return false;
    },
  },
  {
    id: 'core.showProperties',
    tryHandle: (context, event, state) => {
      if (state.dispatchedAction === 'core.showProperties') {
        event.preventDefault();
        context.openPropertiesForActivePane();
        return;
      }
      return false;
    },
  },
  {
    id: 'core.quit',
    tryHandle: (context, event, state) => {
      if (state.dispatchedAction === 'core.quit') {
        if (context.getKeybindingRuntime() !== 'desktop') return;
        event.preventDefault();
        context.quitApplication();
        return;
      }
      return false;
    },
  },
  {
    id: 'core.showShortcutsHelp',
    tryHandle: (context, event, state) => {
      if (state.dispatchedAction === 'core.showShortcutsHelp') {
        event.preventDefault();
        context.setShortcutsHelpOpen(true);
        context.redraw();
        return;
      }
      return false;
    },
  },
  {
    id: 'core.view-in-app',
    tryHandle: (context, event, state) => {
      if (state.dispatchedAction === 'core.view' && !state.forceSystemView) {
        // If a viewer is open *in the active pane*, F3 navigates to the next search match rather
        // than opening/toggling anything - deliberately narrower than `findOpenViewer` (used by
        // Alt+Space/arrow-key paging below): F3 pressed again while the viewer sits in the other,
        // inactive pane must fall through to `resolveViewTarget`/`openViewer`'s own toggle-close-
        // or-switch-file logic below, not hijack the keypress as "next match".
        const workspace = context.getWorkspace();
        const activeViewer =
          workspace === undefined ? undefined : context.getViewer(workspace.activePaneId);
        if (activeViewer !== undefined) {
          event.preventDefault();
          activeViewer.controller.goToNextMatch();
          return;
        }
        // F3 acts on the cursor file regardless of the wider selection. Directories and
        // single-pane workspaces (no opposite pane to open into) fall through
        // to the generic core.view/core.edit/core.openWith block below, which opens the OS default
        // application instead. The viewer itself closes and shows a toast for content that turns
        // out to be binary once its first chunk is fetched, rather than falling back further.
        const target = resolveViewTarget(context);
        if (target !== undefined) {
          event.preventDefault();
          context.openViewer(target.paneId, target.entry, target.initialSearch);
          return;
        }
      }
      return false;
    },
  },
  {
    id: 'core.calculateFolderSize',
    tryHandle: (context, event, state) => {
      if (state.dispatchedAction === 'core.calculateFolderSize') {
        // No backend "cursor entry must be a directory" predicate exists (see action.rs's comment
        // on this action) - files/parent-row are silently ignored here instead.
        const active = context.activeDirectory();
        const selection =
          active === undefined
            ? undefined
            : context.getSelections().get(context.activeTabKey(active.paneId));
        const directory =
          active === undefined
            ? undefined
            : context.getDirectories().get(context.activeTabKey(active.paneId));
        const cursorEntry =
          selection?.cursorEntryId === undefined
            ? undefined
            : directory?.entries.find((entry) => entry.id === selection.cursorEntryId);
        if (
          active !== undefined &&
          cursorEntry !== undefined &&
          cursorEntry.kind === 'directory' &&
          !isParentEntry(cursorEntry.id)
        ) {
          event.preventDefault();
          context.calculateFolderSize(active.paneId, cursorEntry);
          return;
        }
      }
      return false;
    },
  },
  {
    id: 'core.uninstallApplication',
    tryHandle: (context, event, state) => {
      if (state.dispatchedAction === 'core.uninstallApplication') {
        // Mirrors availability.ts's `.app`-suffix check rather than sharing it, matching this
        // file's existing convention for narrow per-action predicates (e.g. calculateFolderSize's
        // directory check above isn't shared with availability.ts either).
        const active = context.activeDirectory();
        const selection =
          active === undefined
            ? undefined
            : context.getSelections().get(context.activeTabKey(active.paneId));
        const directory =
          active === undefined
            ? undefined
            : context.getDirectories().get(context.activeTabKey(active.paneId));
        const cursorEntry =
          selection?.cursorEntryId === undefined
            ? undefined
            : directory?.entries.find((entry) => entry.id === selection.cursorEntryId);
        if (active && cursorEntry?.name.toLowerCase().endsWith('.app')) {
          event.preventDefault();
          context.uninstallApplication(active.paneId, cursorEntry);
        }
        return;
      }
      return false;
    },
  },
  {
    id: 'core.edit-in-app',
    tryHandle: (context, event, state) => {
      if (state.dispatchedAction === 'core.edit' && !state.forceSystemEdit) {
        const active = context.activeDirectory();
        const selection =
          active === undefined
            ? undefined
            : context.getSelections().get(context.activeTabKey(active.paneId));
        const directory =
          active === undefined
            ? undefined
            : context.getDirectories().get(context.activeTabKey(active.paneId));
        const selected = cursorOnlyEntry(selection, directory?.entries ?? []);
        const editEntry = selected?.length === 1 ? selected[0] : undefined;
        const workspace = context.getWorkspace();
        const otherPaneId = workspace?.paneOrder.find((paneId) => paneId !== active?.paneId);
        if (
          editEntry?.kind === 'file' &&
          !isParentEntry(editEntry.id) &&
          otherPaneId !== undefined
        ) {
          event.preventDefault();
          context.openEditor(otherPaneId, editEntry);
          return;
        }
      }
      return false;
    },
  },
  {
    id: 'system-open',
    tryHandle: (context, event, state) => {
      // `state.forceSystemView` (Alt+F3) always resolves to `core.view`, which the backend maps to the
      // same "open with OS default application" behaviour as `core.open` (see PlatformActionKind).
      const viewActionId = state.forceSystemView
        ? 'core.view'
        : state.forceSystemEdit
          ? 'core.edit'
          : state.dispatchedAction;
      if (
        viewActionId === 'core.view' ||
        viewActionId === 'core.edit' ||
        viewActionId === 'core.openWith'
      ) {
        const registeredActions = context.getRegisteredActions();
        const action = registeredActions.find((candidate) => candidate.id === viewActionId);
        // `core.view` itself is never permanently gated (task 0088: its in-app viewer works on
        // every host), but every path that reaches this block dispatches the OS-open fallback
        // instead (directories, multi-selections, single-pane workspaces, forced Alt+F3) - so
        // check `core.open`'s capability, which mirrors what the backend will actually dispatch to.
        const capabilityAction =
          viewActionId === 'core.view'
            ? registeredActions.find((candidate) => candidate.id === 'core.open')
            : action;
        if (capabilityAction?.contextRequirements.featureAvailable === false) {
          // The shortcut is still reachable by keyboard even though its footer
          // hint is hidden (task 0061 follow-up): warn briefly instead of
          // invoking, which would otherwise surface a persistent top-of-screen
          // error from the backend rejecting a known-unavailable action.
          event.preventDefault();
          toast({
            html: t('availability', 'browserUnavailable', {
              action: action?.title ?? t('action', 'view'),
            }),
          });
          return;
        }
        const active = context.activeDirectory();
        const selection =
          active === undefined
            ? undefined
            : context.getSelections().get(context.activeTabKey(active.paneId));
        const directory =
          active === undefined
            ? undefined
            : context.getDirectories().get(context.activeTabKey(active.paneId));
        const selected = cursorOnlyEntry(selection, directory?.entries ?? []);
        const parameters = context.platformActionParameters(
          viewActionId,
          selected ?? [],
          directory?.location,
        );
        if (parameters !== undefined) {
          event.preventDefault();
          context.invokeActionById(viewActionId, parameters, context.actionContext());
        }
        return;
      }
      return false;
    },
  },
] as const satisfies readonly KeydownRoute[];

type GlobalKeydownRouteId =
  | (typeof EARLY_KEYDOWN_ROUTES)[number]['id']
  | (typeof ACTION_KEYDOWN_ROUTES)[number]['id'];

function runKeydownRoutes(
  routes: readonly KeydownRoute[],
  context: GlobalKeydownContext,
  event: KeyboardEvent,
  state: KeydownRouteState,
): GlobalKeydownRouteId | undefined {
  for (const route of routes) {
    if (route.tryHandle(context, event, state) !== false) {
      return route.id as GlobalKeydownRouteId;
    }
  }
  return undefined;
}

export function createGlobalKeydownHandler(
  context: GlobalKeydownContext,
): (event: KeyboardEvent) => void {
  return function handleGlobalKeydown(event: KeyboardEvent): void {
    dispatchGlobalKeydown(context, event);
  };
}

export function dispatchGlobalKeydown(
  context: GlobalKeydownContext,
  event: KeyboardEvent,
): GlobalKeydownRouteId | undefined {
  const initialState: KeydownRouteState = {
    dispatchedAction: undefined,
    forceSystemView: false,
    forceSystemEdit: false,
  };
  const earlyRoute = runKeydownRoutes(EARLY_KEYDOWN_ROUTES, context, event, initialState);
  if (earlyRoute !== undefined) return earlyRoute;

  let dispatchedAction = dispatchKeybinding(
    event,
    {
      scope: isEditableTarget(event.target) ? 'pathInput' : 'table',
      platform: context.getPlatform(),
      runtime: context.getKeybindingRuntime(),
    },
    context.actionsWithFavourites(),
    context.getCurrentSettings()?.keybindings ?? {},
  );
  if (
    !isEditableTarget(event.target) &&
    !event.altKey &&
    !event.ctrlKey &&
    !event.metaKey &&
    (event.key === 'F8' || event.key === 'Delete')
  ) {
    const trashAction = context.getRegisteredActions().find((action) => action.id === 'core.trash');
    const trashAvailable =
      trashAction !== undefined && trashAction.contextRequirements.featureAvailable !== false;
    if (event.shiftKey) {
      dispatchedAction = 'core.delete';
    } else if (trashAvailable && dispatchedAction === 'core.delete') {
      dispatchedAction = 'core.trash';
    }
  }
  // Alt+F3 forces the OS default application instead of the in-app Lister viewer. Although its
  // discoverable action chord belongs to core.open, this route deliberately invokes core.view's
  // platform fallback to preserve the established external-view behaviour.
  const state: KeydownRouteState = {
    dispatchedAction,
    forceSystemView:
      !isEditableTarget(event.target) && event.altKey && event.key.toUpperCase() === 'F3',
    forceSystemEdit:
      !isEditableTarget(event.target) &&
      event.shiftKey &&
      event.altKey &&
      !event.ctrlKey &&
      !event.metaKey &&
      event.key.toUpperCase() === 'F4',
  };
  return runKeydownRoutes(ACTION_KEYDOWN_ROUTES, context, event, state);
}

/** Cross-platform embedded-terminal chord, with F12 reserved for the desktop host. */
export function isTerminalToggleShortcut(
  event: Pick<KeyboardEvent, 'key' | 'code' | 'altKey' | 'ctrlKey' | 'metaKey' | 'shiftKey'>,
  runtime: KeybindingRuntime,
): boolean {
  const bareModifiers = !event.altKey && !event.metaKey && !event.shiftKey;
  const backquoteKey = event.key === '`' || event.code === 'Backquote';
  const f12Key = event.key === 'F12' || event.code === 'F12';
  return (
    runtime === 'desktop' &&
    ((bareModifiers && event.ctrlKey && backquoteKey) ||
      (bareModifiers && !event.ctrlKey && f12Key))
  );
}
