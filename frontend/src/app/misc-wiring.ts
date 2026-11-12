import m from 'mithril';
import type { ClipboardState } from '../features/clipboard/clipboard';
import { emptyClipboard } from '../features/clipboard/clipboard';
import { getSelectedEntriesOrCursor } from '../features/selection/selection';
import { t } from '../i18n';
import { type FunctionKeyModifiers, hasPrimaryModifier } from '../keybindings/dispatcher';
import type { Location, PaneId } from '../models';
import { applyAppPatches, clipboardPatch } from '../state';
import {
  labelWithShortcut as formatLabelWithShortcut,
  displayShortcut as formatShortcutForPlatform,
  tabKey,
} from './shell-helpers';

/** Sets up the miscWiring feature wiring and returns its functions. */
import type { ShellState } from './shell-state';

/** The narrow cross-cutting surface clipboard/key-chord/viewer-scroll wiring needs from the shell. */
export interface MiscCore {
  readonly activeDirectory: () =>
    | { readonly paneId: PaneId; readonly location: Location }
    | undefined;
  readonly activeTabKey: (paneId: PaneId) => string;
}

export function createMiscWiring(shellState: ShellState, core: MiscCore) {
  function clipboard() {
    return shellState.appState?.clipboard ?? emptyClipboard;
  }

  function replaceClipboard(next = emptyClipboard): void {
    if (shellState.appState !== undefined) {
      shellState.appState = applyAppPatches(shellState.appState, clipboardPatch(next));
    }
    shellState.osFileClipboard.mirror(next);
  }

  function resolvePasteClipboard(): Promise<ClipboardState> {
    return shellState.osFileClipboard.resolvePasteClipboard(clipboard());
  }

  function selectedLocations(): readonly Location[] {
    const active = core.activeDirectory();
    const directory =
      active === undefined
        ? undefined
        : shellState.directories.get(core.activeTabKey(active.paneId));
    const selection =
      active === undefined
        ? undefined
        : shellState.selections.get(core.activeTabKey(active.paneId));
    return getSelectedEntriesOrCursor(selection, directory?.entries ?? []).map(
      (entry) => entry.location,
    );
  }

  function invokeFunctionKeyShortcut(shortcut: string): void {
    const parts = shortcut.split('+');
    const key = parts.pop();
    if (key === undefined) return;
    const primary = parts.includes('CTRL');
    const paneElement = document.querySelector<HTMLElement>('[data-active="true"] > .fm-pane');
    paneElement?.dispatchEvent(
      new KeyboardEvent('keydown', {
        key,
        ctrlKey: primary && shellState.platform !== 'macos',
        metaKey: primary && shellState.platform === 'macos',
        altKey: parts.includes('ALT'),
        shiftKey: parts.includes('SHIFT'),
        bubbles: true,
      }),
    );
  }

  function updateFunctionKeyModifiers(event: KeyboardEvent): void {
    const next: FunctionKeyModifiers = {
      primary: hasPrimaryModifier(event, shellState.platform),
      alt: event.altKey,
      shift: event.shiftKey,
    };
    if (
      next.primary === shellState.functionKeyModifiers.primary &&
      next.alt === shellState.functionKeyModifiers.alt &&
      next.shift === shellState.functionKeyModifiers.shift
    )
      return;
    shellState.functionKeyModifiers = next;
    m.redraw();
  }

  function resetFunctionKeyModifiers(): void {
    shellState.functionKeyModifiers = {};
    m.redraw();
  }

  function functionKeyTitle(binding: {
    readonly actionId: string;
    readonly shortcut: string;
    readonly title: string;
  }): string {
    if (binding.actionId === 'core.showShortcutsHelp') return t('menu', 'help');
    if (binding.shortcut === 'ALT+F3') return t('action', 'openExternally');
    if (binding.shortcut === 'ALT+SHIFT+F4') return t('action', 'externalEdit');
    return binding.title;
  }

  function displayShortcut(shortcut: string): string {
    return formatShortcutForPlatform(shortcut, shellState.platform);
  }

  function labelWithShortcut(label: string, shortcut: string | undefined): string {
    return formatLabelWithShortcut(label, shortcut, shellState.platform);
  }

  function openViewerPaneId(): PaneId | undefined {
    if (shellState.workspace === undefined) return undefined;
    for (const paneId of shellState.workspace.paneOrder) {
      const tabId = shellState.workspace.panesById[paneId]?.activeTabId;
      if (tabId !== undefined && shellState.viewerByTab.get(tabKey(paneId, tabId)) !== undefined) {
        return paneId;
      }
    }
    return undefined;
  }

  function focusViewer(paneId: PaneId): void {
    const root = document.querySelector<HTMLElement>(`[data-pane-id="${paneId}"]`);
    const readingSurface = root?.querySelector<HTMLElement>('[data-viewer-focus-target]');
    const searchInput = root?.querySelector<HTMLInputElement>('.fm-file-viewer-search-input');
    const section = root?.querySelector<HTMLElement>('.fm-pane-viewer');
    (readingSurface ?? searchInput ?? section)?.focus();
  }

  function focusViewerSearch(paneId: PaneId): void {
    const root = document.querySelector<HTMLElement>(`[data-pane-id="${paneId}"]`);
    root
      ?.querySelector<HTMLElement>('.fm-file-viewer')
      ?.dispatchEvent(new CustomEvent('fm-viewer-toggle-search'));
    root?.querySelector<HTMLInputElement>('.fm-file-viewer-search-input')?.focus();
  }

  const VIEWER_SCROLL_LINE_STEP_PX = 48;

  const VIEWER_SCROLL_PAGE_FRACTION = 0.9;

  function scrollViewer(
    paneId: PaneId,
    dx: -1 | 0 | 1,
    dy: -1 | 0 | 1,
    unit: 'line' | 'page',
  ): void {
    const container = document.querySelector<HTMLElement>(
      `[data-pane-id="${paneId}"] .fm-file-viewer-body`,
    );
    if (container === null) return;
    const stepX =
      unit === 'page'
        ? container.clientWidth * VIEWER_SCROLL_PAGE_FRACTION
        : VIEWER_SCROLL_LINE_STEP_PX;
    const stepY =
      unit === 'page'
        ? container.clientHeight * VIEWER_SCROLL_PAGE_FRACTION
        : VIEWER_SCROLL_LINE_STEP_PX;
    container.scrollBy({ left: dx * stepX, top: dy * stepY });
  }

  return {
    clipboard,
    replaceClipboard,
    resolvePasteClipboard,
    selectedLocations,
    invokeFunctionKeyShortcut,
    updateFunctionKeyModifiers,
    resetFunctionKeyModifiers,
    functionKeyTitle,
    displayShortcut,
    labelWithShortcut,
    openViewerPaneId,
    focusViewer,
    focusViewerSearch,
    VIEWER_SCROLL_LINE_STEP_PX,
    VIEWER_SCROLL_PAGE_FRACTION,
    scrollViewer,
  };
}
