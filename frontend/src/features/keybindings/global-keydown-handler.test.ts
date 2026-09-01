import { describe, expect, it, vi } from 'vitest';
import type {
  ActionDescriptor,
  EntrySummary,
  Location,
  PaneId,
  WorkspaceProjection,
} from '../../models';
import type { NavigationController, PaneDirectoryView } from '../navigation/navigation';
import type { OperationsController } from '../operations/operations-controller';
import type { TabController } from '../panes/tab-controller';
import type { SelectionState } from '../selection/selection';
import {
  createGlobalKeydownHandler,
  dispatchGlobalKeydown,
  type GlobalKeydownContext,
} from './global-keydown-handler';

const ACTIONS: readonly ActionDescriptor[] = [
  { id: 'core.rootDirectory', title: 'Root', defaultShortcuts: [{ key: 'Backspace', ctrl: true }] },
  {
    id: 'core.openInNewTab',
    title: 'Open in new tab',
    defaultShortcuts: [{ key: 'ArrowUp', ctrl: true }],
  },
  {
    id: 'core.openInNewTabOtherPane',
    title: 'Open in new tab (other pane)',
    defaultShortcuts: [{ key: 'ArrowUp', ctrl: true, shift: true }],
  },
  {
    id: 'core.duplicateLocationToOtherPane',
    title: 'Duplicate directory',
    defaultShortcuts: [
      { key: 'ArrowLeft', ctrl: true },
      { key: 'ArrowRight', ctrl: true },
    ],
  },
  { id: 'core.swapPanes', title: 'Swap panes', defaultShortcuts: [{ key: 'u', ctrl: true }] },
  {
    id: 'core.compareDirectories',
    title: 'Compare directories',
    defaultShortcuts: [{ key: 'F2', shift: true }],
  },
  {
    id: 'core.calculateChecksum',
    title: 'Calculate checksums',
    defaultShortcuts: [{ key: 'F9', shift: true }],
  },
  {
    id: 'core.findDuplicates',
    title: 'Find duplicate files',
    defaultShortcuts: [{ key: 'F9', ctrl: true }],
  },
  {
    id: 'client.diskUsage',
    title: 'Disk usage',
    defaultShortcuts: [{ key: 'l', ctrl: true, shift: true }],
  },
  {
    id: 'core.swapPaneTabs',
    title: 'Swap pane tabs',
    defaultShortcuts: [{ key: 'u', ctrl: true, shift: true }],
  },
  {
    id: 'core.closeAllTabs',
    title: 'Close all tabs',
    defaultShortcuts: [{ key: 'w', ctrl: true, shift: true }],
  },
  {
    id: 'core.newConnection',
    title: 'New connection',
    defaultShortcuts: [{ key: 'n', ctrl: true }],
  },
  {
    id: 'core.reactivateQuickFilter',
    title: 'Reactivate quick filter',
    defaultShortcuts: [{ key: 's', ctrl: true, shift: true }],
  },
  {
    id: 'core.clearQuickFilter',
    title: 'Show all files',
    defaultShortcuts: [{ key: 'F10', ctrl: true }],
  },
  { id: 'core.sortByName', title: 'Sort by name', defaultShortcuts: [{ key: 'F3', ctrl: true }] },
  { id: 'core.createFile', title: 'New file', defaultShortcuts: [{ key: 'F4', shift: true }] },
  { id: 'core.duplicate', title: 'Duplicate', defaultShortcuts: [{ key: 'F5', shift: true }] },
  {
    id: 'core.openMultiRename',
    title: 'Multi-rename',
    defaultShortcuts: [{ key: 'm', ctrl: true }],
  },
  {
    id: 'core.showProperties',
    title: 'Properties',
    defaultShortcuts: [{ key: 'Enter', alt: true }],
  },
  { id: 'core.quit', title: 'Quit', defaultShortcuts: [{ key: 'F4', alt: true }] },
  { id: 'core.showShortcutsHelp', title: 'Shortcuts', defaultShortcuts: [{ key: 'F1' }] },
  {
    id: 'core.calculateFolderSize',
    title: 'Calculate folder size',
    defaultShortcuts: [{ key: '.', ctrl: true }],
  },
  {
    id: 'core.uninstallApplication',
    title: 'Uninstall Application…',
    defaultShortcuts: [{ key: 'u', ctrl: true, alt: true }],
  },
].map(
  (action): ActionDescriptor => ({
    category: 'test',
    contextRequirements: {},
    source: { kind: 'core' },
    ...action,
  }),
);

const PANE_A = 'pane-a' as PaneId;
const PANE_B = 'pane-b' as PaneId;

function workspace(overrides: Partial<WorkspaceProjection> = {}): WorkspaceProjection {
  return {
    id: 'workspace-1',
    name: 'Workspace',
    revision: 1,
    layout: {
      type: 'split',
      axis: 'horizontal',
      ratio: 0.5,
      first: { type: 'pane', paneId: PANE_A },
      second: { type: 'pane', paneId: PANE_B },
    },
    paneOrder: [PANE_A, PANE_B],
    panesById: {
      [PANE_A]: {
        id: PANE_A,
        tabOrder: ['tab-a' as never],
        tabsById: { 'tab-a': { id: 'tab-a' as never } } as never,
        activeTabId: 'tab-a' as never,
      },
      [PANE_B]: {
        id: PANE_B,
        tabOrder: ['tab-b' as never],
        tabsById: { 'tab-b': { id: 'tab-b' as never } } as never,
        activeTabId: 'tab-b' as never,
      },
    },
    activePaneId: PANE_A,
    operationCentre: { visible: false, height: 180 },
    ephemeral: false,
    ...overrides,
  };
}

function makeContext(overrides: Partial<GlobalKeydownContext> = {}): GlobalKeydownContext {
  const base: GlobalKeydownContext = {
    getCommandPaletteOpen: () => false,
    getPlatform: () => 'linux',
    getKeybindingRuntime: () => 'browser',
    getCurrentSettings: () => undefined,
    getWorkspace: () => workspace(),
    getSelections: () => new Map<string, SelectionState>(),
    getDirectories: () => new Map<string, PaneDirectoryView>(),
    getRegisteredActions: () => ACTIONS,
    clipboard: () => ({ locations: [] }),
    getFindFilesOpen: () => false,
    getViewer: () => undefined,
    getArchiveCreateRequest: () => undefined,
    getCreateDirectoryOpen: () => false,
    getCreateFileOpen: () => false,
    getAppState: () => undefined,
    getLastQuickFilterQuery: () => undefined,
    getShortcutsHelpOpen: () => false,
    setCommandPaletteOpen: vi.fn(),
    setClipboardMessage: vi.fn(),
    setArchiveCreateRequest: vi.fn(),
    setCreateDirectoryOpen: vi.fn(),
    setCreateFileOpen: vi.fn(),
    setAppState: vi.fn(),
    setQuickFilterOpen: vi.fn(),
    setActiveTabQuickFilter: vi.fn(),
    setConnectionsManagerOpen: vi.fn(),
    setShortcutsHelpOpen: vi.fn(),
    getTabController: () =>
      ({
        openTabAt: vi.fn(),
        closeAllTabs: vi.fn(),
      }) as unknown as TabController,
    getOpsController: () =>
      ({
        duplicate: vi.fn().mockResolvedValue({}),
      }) as unknown as OperationsController,
    getNavigation: () =>
      ({
        navigate: vi.fn().mockResolvedValue(undefined),
      }) as unknown as NavigationController,
    activeDirectory: () => ({
      paneId: PANE_A,
      location: { providerId: 'local', uri: 'file:///a/b/c' },
    }),
    activeTabKey: (paneId) => `${paneId}:tab`,
    actionsWithFavourites: () => ACTIONS,
    openFindFiles: vi.fn(),
    replaceClipboard: vi.fn(),
    selectedLocations: () => [],
    invokeActionById: vi.fn(),
    openViewer: vi.fn(),
    openEditor: vi.fn(),
    calculateFolderSize: vi.fn(),
    uninstallApplication: vi.fn(),
    openSettingsDialog: vi.fn(),
    actionContext: () => ({ selectedEntryIds: [] }),
    commandAvailabilityContext: () => ({}) as never,
    contentSearchInitialQuery: () => undefined,
    refetchAffectedPanes: vi.fn(),
    platformActionParameters: () => undefined,
    activatePane: vi.fn(),
    focusPane: vi.fn(),
    focusViewer: vi.fn(),
    scrollViewer: vi.fn(),
    redraw: vi.fn(),
    toggleTerminal: vi.fn(),
    toggleDirectoryTree: vi.fn(),
    toggleOperationCentre: vi.fn(),
    setSort: vi.fn(),
    swapPaneTabSets: vi.fn(),
    openMultiRenameForActivePane: vi.fn(),
    openPropertiesForActivePane: vi.fn(),
    quitApplication: vi.fn(),
    startComparison: vi.fn(),
    calculateChecksums: vi.fn(),
    findDuplicates: vi.fn(),
    openDiskUsage: vi.fn(),
  };
  return { ...base, ...overrides };
}

function defaultCode(key: string): string {
  if (/^[a-zA-Z]$/u.test(key)) return `Key${key.toUpperCase()}`;
  if (/^[0-9]$/u.test(key)) return `Digit${key}`;
  return key;
}

function keydown(key: string, modifiers: Partial<KeyboardEventInit> = {}): KeyboardEvent {
  return new KeyboardEvent('keydown', {
    key,
    code: defaultCode(key),
    bubbles: true,
    cancelable: true,
    ...modifiers,
  });
}

/** Dispatches a keydown as if it originated from inside an open F3 viewer's DOM subtree (see
 * `isWithinViewer`), by actually firing it on a `.fm-pane-viewer` element rather than just
 * constructing it - a plain `keydown()` event's `target` stays `null` until dispatched. `target`
 * lets a test fire from a specific descendant, e.g. the search input or the CodeMirror body,
 * instead of the section itself. */
function viewerKeydown(
  key: string,
  modifiers: Partial<KeyboardEventInit> = {},
  target?: (section: HTMLElement) => HTMLElement,
): KeyboardEvent {
  const section = document.createElement('section');
  section.className = 'fm-pane-viewer';
  document.body.append(section);
  const dispatchTarget = target?.(section) ?? section;
  const event = keydown(key, modifiers);
  dispatchTarget.dispatchEvent(event);
  section.remove();
  return event;
}

describe('dispatchGlobalKeydown precedence', () => {
  it('routes Alt+Z to the operation centre toggle', () => {
    const toggleOperationCentre = vi.fn();
    const context = makeContext({ toggleOperationCentre });

    const event = keydown('z', { altKey: true });
    const route = dispatchGlobalKeydown(context, event);

    expect(route).toBe('operation-centre-toggle');
    expect(event.defaultPrevented).toBe(true);
    expect(toggleOperationCentre).toHaveBeenCalledOnce();
  });

  it('routes Alt+Z by physical key when Option modifies the character on macOS', () => {
    const toggleOperationCentre = vi.fn();
    const context = makeContext({ toggleOperationCentre });

    const event = keydown('Ω', { code: 'KeyZ', altKey: true });
    const route = dispatchGlobalKeydown(context, event);

    expect(route).toBe('operation-centre-toggle');
    expect(event.defaultPrevented).toBe(true);
    expect(toggleOperationCentre).toHaveBeenCalledOnce();
  });

  it('routes Alt+F10 to the directory tree before registered actions', () => {
    const toggleDirectoryTree = vi.fn();
    const setActiveTabQuickFilter = vi.fn();
    const context = makeContext({ toggleDirectoryTree, setActiveTabQuickFilter });

    const route = dispatchGlobalKeydown(context, keydown('F10', { altKey: true }));

    expect(route).toBe('directory-tree-toggle');
    expect(toggleDirectoryTree).toHaveBeenCalledOnce();
    expect(setActiveTabQuickFilter).not.toHaveBeenCalled();
  });

  it('routes a terminal shortcut before the open command palette blocker', () => {
    const toggleTerminal = vi.fn();
    const context = makeContext({
      getCommandPaletteOpen: () => true,
      getKeybindingRuntime: () => 'desktop',
      toggleTerminal,
    });

    const route = dispatchGlobalKeydown(context, keydown('F12'));

    expect(route).toBe('terminal-toggle');
    expect(toggleTerminal).toHaveBeenCalledOnce();
  });
});

describe('createGlobalKeydownHandler - task 0128 shortcuts', () => {
  it('Ctrl+Backspace navigates to the root of the active location', () => {
    const navigate = vi.fn().mockResolvedValue(undefined);
    const context = makeContext({
      getNavigation: () => ({ navigate }) as unknown as NavigationController,
    });
    createGlobalKeydownHandler(context)(keydown('Backspace', { ctrlKey: true }));
    expect(navigate).toHaveBeenCalledWith(PANE_A, { providerId: 'local', uri: 'file:///' });
  });

  it('Ctrl+Up opens the directory under the cursor as a new tab in the same pane', () => {
    const openTabAt = vi.fn();
    const cursorDir: EntrySummary = {
      id: 'dir-1' as never,
      location: { providerId: 'local', uri: 'file:///a/dir' },
      name: 'dir',
      kind: 'directory',
      hidden: false,
      readOnly: false,
      metadataRevision: 0,
    };
    const context = makeContext({
      getTabController: () => ({ openTabAt, closeAllTabs: vi.fn() }) as unknown as TabController,
      getSelections: () =>
        new Map([['pane-a:tab', { selectedEntryIds: [], cursorEntryId: 'dir-1' as never }]]),
      getDirectories: () =>
        new Map([['pane-a:tab', { entries: [cursorDir] } as unknown as PaneDirectoryView]]),
    });
    createGlobalKeydownHandler(context)(keydown('ArrowUp', { ctrlKey: true }));
    expect(openTabAt).toHaveBeenCalledWith(PANE_A, cursorDir.location);
  });

  it('Ctrl+Shift+Up opens the directory under the cursor as a new tab in the other pane', () => {
    const openTabAt = vi.fn();
    const cursorDir: EntrySummary = {
      id: 'dir-1' as never,
      location: { providerId: 'local', uri: 'file:///a/dir' },
      name: 'dir',
      kind: 'directory',
      hidden: false,
      readOnly: false,
      metadataRevision: 0,
    };
    const context = makeContext({
      getTabController: () => ({ openTabAt, closeAllTabs: vi.fn() }) as unknown as TabController,
      getSelections: () =>
        new Map([['pane-a:tab', { selectedEntryIds: [], cursorEntryId: 'dir-1' as never }]]),
      getDirectories: () =>
        new Map([['pane-a:tab', { entries: [cursorDir] } as unknown as PaneDirectoryView]]),
    });
    createGlobalKeydownHandler(context)(keydown('ArrowUp', { ctrlKey: true, shiftKey: true }));
    expect(openTabAt).toHaveBeenCalledWith(PANE_B, cursorDir.location);
  });

  it('Ctrl+Left duplicates the active location into the other pane', () => {
    const navigate = vi.fn().mockResolvedValue(undefined);
    const context = makeContext({
      getNavigation: () => ({ navigate }) as unknown as NavigationController,
    });
    createGlobalKeydownHandler(context)(keydown('ArrowLeft', { ctrlKey: true }));
    expect(navigate).toHaveBeenCalledWith(PANE_B, { providerId: 'local', uri: 'file:///a/b/c' });
  });

  it('Ctrl+U swaps the two panes active locations (desktop runtime - Ctrl+U is browser-reserved)', () => {
    const navigate = vi.fn().mockResolvedValue(undefined);
    const locA: Location = { providerId: 'local', uri: 'file:///left' };
    const locB: Location = { providerId: 'local', uri: 'file:///right' };
    const context = makeContext({
      getKeybindingRuntime: () => 'desktop',
      getNavigation: () => ({ navigate }) as unknown as NavigationController,
      getDirectories: () =>
        new Map([
          ['pane-a:tab', { location: locA } as unknown as PaneDirectoryView],
          ['pane-b:tab', { location: locB } as unknown as PaneDirectoryView],
        ]),
    });
    createGlobalKeydownHandler(context)(keydown('u', { ctrlKey: true }));
    expect(navigate).toHaveBeenCalledWith(PANE_A, locB);
    expect(navigate).toHaveBeenCalledWith(PANE_B, locA);
  });

  it('Ctrl+U does nothing in browser runtime (Chrome reserves it for View Source)', () => {
    const navigate = vi.fn().mockResolvedValue(undefined);
    const context = makeContext({
      getKeybindingRuntime: () => 'browser',
      getNavigation: () => ({ navigate }) as unknown as NavigationController,
    });
    createGlobalKeydownHandler(context)(keydown('u', { ctrlKey: true }));
    expect(navigate).not.toHaveBeenCalled();
  });

  it('Ctrl+Shift+U swaps the two panes tab sets', () => {
    const swapPaneTabSets = vi.fn();
    const context = makeContext({ swapPaneTabSets });
    createGlobalKeydownHandler(context)(keydown('u', { ctrlKey: true, shiftKey: true }));
    expect(swapPaneTabSets).toHaveBeenCalledWith(PANE_A, PANE_B);
  });

  it('Ctrl+Shift+W closes every tab except the active one', () => {
    const closeAllTabs = vi.fn();
    const context = makeContext({
      getTabController: () => ({ openTabAt: vi.fn(), closeAllTabs }) as unknown as TabController,
    });
    createGlobalKeydownHandler(context)(keydown('w', { ctrlKey: true, shiftKey: true }));
    expect(closeAllTabs).toHaveBeenCalledWith(PANE_A);
  });

  it('Ctrl+N opens the new-connection dialog (desktop runtime - Ctrl+N is browser-reserved)', () => {
    const setConnectionsManagerOpen = vi.fn();
    const context = makeContext({
      getKeybindingRuntime: () => 'desktop',
      setConnectionsManagerOpen,
    });
    createGlobalKeydownHandler(context)(keydown('n', { ctrlKey: true }));
    expect(setConnectionsManagerOpen).toHaveBeenCalledWith(true);
  });

  it('Ctrl+N does nothing in browser runtime (Chrome reserves it for a new window)', () => {
    const setConnectionsManagerOpen = vi.fn();
    const context = makeContext({
      getKeybindingRuntime: () => 'browser',
      setConnectionsManagerOpen,
    });
    createGlobalKeydownHandler(context)(keydown('n', { ctrlKey: true }));
    expect(setConnectionsManagerOpen).not.toHaveBeenCalled();
  });

  it('Ctrl+Shift+S reactivates the last non-empty Quick Filter query', () => {
    const setActiveTabQuickFilter = vi.fn();
    const context = makeContext({
      getLastQuickFilterQuery: () => 'report',
      setActiveTabQuickFilter,
    });
    createGlobalKeydownHandler(context)(keydown('s', { ctrlKey: true, shiftKey: true }));
    expect(setActiveTabQuickFilter).toHaveBeenCalledWith(PANE_A, 'report');
  });

  it('Ctrl+Shift+S does nothing when no prior query was cached', () => {
    const setActiveTabQuickFilter = vi.fn();
    const context = makeContext({
      getLastQuickFilterQuery: () => undefined,
      setActiveTabQuickFilter,
    });
    createGlobalKeydownHandler(context)(keydown('s', { ctrlKey: true, shiftKey: true }));
    expect(setActiveTabQuickFilter).not.toHaveBeenCalled();
  });

  it('Ctrl+F10 clears the active Quick Filter', () => {
    const setActiveTabQuickFilter = vi.fn();
    const context = makeContext({ setActiveTabQuickFilter });
    createGlobalKeydownHandler(context)(keydown('F10', { ctrlKey: true }));
    expect(setActiveTabQuickFilter).toHaveBeenCalledWith(PANE_A, undefined);
  });

  it('Alt+F10 toggles the directory-tree sidebar without also clearing the Quick Filter', () => {
    const toggleDirectoryTree = vi.fn();
    const setActiveTabQuickFilter = vi.fn();
    const context = makeContext({ toggleDirectoryTree, setActiveTabQuickFilter });
    createGlobalKeydownHandler(context)(keydown('F10', { altKey: true }));
    expect(toggleDirectoryTree).toHaveBeenCalledTimes(1);
    expect(setActiveTabQuickFilter).not.toHaveBeenCalled();
  });

  it('Ctrl+F3 sorts the active pane by name', () => {
    const setSort = vi.fn();
    const context = makeContext({ setSort });
    createGlobalKeydownHandler(context)(keydown('F3', { ctrlKey: true }));
    expect(setSort).toHaveBeenCalledWith(PANE_A, [
      { columnId: 'core.name', direction: 'ascending' },
    ]);
  });

  it('Shift+F4 opens the create-file dialog', () => {
    const setCreateFileOpen = vi.fn();
    const context = makeContext({ setCreateFileOpen });
    createGlobalKeydownHandler(context)(keydown('F4', { shiftKey: true }));
    expect(setCreateFileOpen).toHaveBeenCalledWith(true);
  });

  it('Shift+F5 duplicates the selected entries', async () => {
    const duplicate = vi.fn().mockResolvedValue({});
    const src: Location = { providerId: 'local', uri: 'file:///a.txt' };
    const context = makeContext({
      selectedLocations: () => [src],
      getOpsController: () => ({ duplicate }) as unknown as OperationsController,
    });
    createGlobalKeydownHandler(context)(keydown('F5', { shiftKey: true }));
    await Promise.resolve();
    expect(duplicate).toHaveBeenCalledWith([src]);
  });

  it('Ctrl+M opens the Multi-Rename Tool directly', () => {
    const openMultiRenameForActivePane = vi.fn();
    const context = makeContext({ openMultiRenameForActivePane });
    createGlobalKeydownHandler(context)(keydown('m', { ctrlKey: true }));
    expect(openMultiRenameForActivePane).toHaveBeenCalled();
  });

  it('Alt+Enter opens Properties for the active pane', () => {
    const openPropertiesForActivePane = vi.fn();
    const context = makeContext({ openPropertiesForActivePane });
    createGlobalKeydownHandler(context)(keydown('Enter', { altKey: true }));
    expect(openPropertiesForActivePane).toHaveBeenCalled();
  });

  it('Alt+F4 quits in desktop runtime', () => {
    const quitApplication = vi.fn();
    const context = makeContext({ getKeybindingRuntime: () => 'desktop', quitApplication });
    createGlobalKeydownHandler(context)(keydown('F4', { altKey: true }));
    expect(quitApplication).toHaveBeenCalled();
  });

  it('Alt+F4 is a no-op in browser runtime', () => {
    const quitApplication = vi.fn();
    const context = makeContext({ getKeybindingRuntime: () => 'browser', quitApplication });
    createGlobalKeydownHandler(context)(keydown('F4', { altKey: true }));
    expect(quitApplication).not.toHaveBeenCalled();
  });

  it('Shift+F2 starts a directory comparison', () => {
    const startComparison = vi.fn();
    const context = makeContext({ startComparison });
    createGlobalKeydownHandler(context)(keydown('F2', { shiftKey: true }));
    expect(startComparison).toHaveBeenCalled();
  });

  it('dispatches the checksum command to the checksum controller (task 0077)', () => {
    const calculateChecksums = vi.fn();
    const context = makeContext({ calculateChecksums });
    createGlobalKeydownHandler(context)(keydown('F9', { shiftKey: true }));
    expect(calculateChecksums).toHaveBeenCalled();
  });

  it('dispatches the find-duplicates command to the checksum controller (task 0077)', () => {
    const findDuplicates = vi.fn();
    const context = makeContext({ findDuplicates });
    createGlobalKeydownHandler(context)(keydown('F9', { ctrlKey: true }));
    expect(findDuplicates).toHaveBeenCalled();
  });

  it('Ctrl+Shift+L opens disk usage for the active pane in a separate tab', () => {
    const openDiskUsage = vi.fn();
    const context = makeContext({ openDiskUsage });
    createGlobalKeydownHandler(context)(keydown('l', { ctrlKey: true, shiftKey: true }));
    expect(openDiskUsage).toHaveBeenCalled();
  });

  it('F1 opens the shortcuts help overlay', () => {
    const setShortcutsHelpOpen = vi.fn();
    const context = makeContext({ setShortcutsHelpOpen });
    createGlobalKeydownHandler(context)(keydown('F1'));
    expect(setShortcutsHelpOpen).toHaveBeenCalledWith(true);
  });

  it('Ctrl+, opens the Settings dialog', () => {
    const openSettingsDialog = vi.fn();
    const context = makeContext({ openSettingsDialog });
    createGlobalKeydownHandler(context)(keydown(',', { ctrlKey: true }));
    expect(openSettingsDialog).toHaveBeenCalled();
  });

  it('Ctrl+. calculates the folder size for a directory under the cursor', () => {
    const calculateFolderSize = vi.fn();
    const cursorDir: EntrySummary = {
      id: 'dir-1' as never,
      location: { providerId: 'local', uri: 'file:///a/dir' },
      name: 'dir',
      kind: 'directory',
      hidden: false,
      readOnly: false,
      metadataRevision: 0,
    };
    const context = makeContext({
      calculateFolderSize,
      getSelections: () =>
        new Map([['pane-a:tab', { selectedEntryIds: [], cursorEntryId: 'dir-1' as never }]]),
      getDirectories: () =>
        new Map([['pane-a:tab', { entries: [cursorDir] } as unknown as PaneDirectoryView]]),
    });
    createGlobalKeydownHandler(context)(keydown('.', { ctrlKey: true }));
    expect(calculateFolderSize).toHaveBeenCalledWith(PANE_A, cursorDir);
  });

  it('Alt+Space calculates the folder size for a directory under the cursor', () => {
    const calculateFolderSize = vi.fn();
    const cursorDir: EntrySummary = {
      id: 'dir-1' as never,
      location: { providerId: 'local', uri: 'file:///a/dir' },
      name: 'dir',
      kind: 'directory',
      hidden: false,
      readOnly: false,
      metadataRevision: 0,
    };
    const context = makeContext({
      calculateFolderSize,
      getSelections: () =>
        new Map([['pane-a:tab', { selectedEntryIds: [], cursorEntryId: 'dir-1' as never }]]),
      getDirectories: () =>
        new Map([['pane-a:tab', { entries: [cursorDir] } as unknown as PaneDirectoryView]]),
    });
    createGlobalKeydownHandler(context)(keydown(' ', { altKey: true, code: 'Space' }));
    expect(calculateFolderSize).toHaveBeenCalledWith(PANE_A, cursorDir);
  });

  it('Ctrl+. does nothing when the cursor is on a file rather than a directory', () => {
    const calculateFolderSize = vi.fn();
    const cursorFile: EntrySummary = {
      id: 'file-1' as never,
      location: { providerId: 'local', uri: 'file:///a/report.txt' },
      name: 'report.txt',
      kind: 'file',
      hidden: false,
      readOnly: false,
      metadataRevision: 0,
    };
    const context = makeContext({
      calculateFolderSize,
      getSelections: () =>
        new Map([['pane-a:tab', { selectedEntryIds: [], cursorEntryId: 'file-1' as never }]]),
      getDirectories: () =>
        new Map([['pane-a:tab', { entries: [cursorFile] } as unknown as PaneDirectoryView]]),
    });
    createGlobalKeydownHandler(context)(keydown('.', { ctrlKey: true }));
    expect(calculateFolderSize).not.toHaveBeenCalled();
  });

  it('dispatches uninstallApplication for a .app-suffixed cursor entry', () => {
    const uninstallApplication = vi.fn();
    const cursorApp: EntrySummary = {
      id: 'app-1' as never,
      location: { providerId: 'local', uri: 'file:///Applications/Widget.app' },
      name: 'Widget.app',
      kind: 'file',
      hidden: false,
      readOnly: false,
      metadataRevision: 0,
    };
    const context = makeContext({
      uninstallApplication,
      getSelections: () =>
        new Map([['pane-a:tab', { selectedEntryIds: [], cursorEntryId: 'app-1' as never }]]),
      getDirectories: () =>
        new Map([['pane-a:tab', { entries: [cursorApp] } as unknown as PaneDirectoryView]]),
    });
    createGlobalKeydownHandler(context)(keydown('u', { ctrlKey: true, altKey: true }));
    expect(uninstallApplication).toHaveBeenCalledWith(PANE_A, cursorApp);
  });

  it('does nothing for uninstallApplication when the cursor is not on a .app entry', () => {
    const uninstallApplication = vi.fn();
    const cursorFile: EntrySummary = {
      id: 'file-1' as never,
      location: { providerId: 'local', uri: 'file:///a/report.txt' },
      name: 'report.txt',
      kind: 'file',
      hidden: false,
      readOnly: false,
      metadataRevision: 0,
    };
    const context = makeContext({
      uninstallApplication,
      getSelections: () =>
        new Map([['pane-a:tab', { selectedEntryIds: [], cursorEntryId: 'file-1' as never }]]),
      getDirectories: () =>
        new Map([['pane-a:tab', { entries: [cursorFile] } as unknown as PaneDirectoryView]]),
    });
    createGlobalKeydownHandler(context)(keydown('u', { ctrlKey: true, altKey: true }));
    expect(uninstallApplication).not.toHaveBeenCalled();
  });

  it('Alt+Space toggles the metadata panel when a viewer is open in the active pane', () => {
    const toggleMetadataPanel = vi.fn();
    const context = makeContext({
      getViewer: (paneId) =>
        paneId === PANE_A
          ? ({ controller: { toggleMetadataPanel } as never, state: {} as never } as never)
          : undefined,
    });
    const event = keydown(' ', { altKey: true, code: 'Space' });
    const preventDefault = vi.spyOn(event, 'preventDefault');
    createGlobalKeydownHandler(context)(event);
    expect(toggleMetadataPanel).toHaveBeenCalledTimes(1);
    expect(preventDefault).toHaveBeenCalled();
  });

  it('Alt+Space toggles the metadata panel of a viewer open in the inactive pane (no pane switch needed)', () => {
    const toggleMetadataPanel = vi.fn();
    const context = makeContext({
      // Default active pane is PANE_A; the viewer lives in PANE_B, mirroring F3 opening the
      // viewer in the *opposite* pane from the one the user pressed F3 in.
      getViewer: (paneId) =>
        paneId === PANE_B
          ? ({ controller: { toggleMetadataPanel } as never, state: {} as never } as never)
          : undefined,
    });
    const event = keydown(' ', { altKey: true, code: 'Space' });
    createGlobalKeydownHandler(context)(event);
    expect(toggleMetadataPanel).toHaveBeenCalledTimes(1);
  });

  it('ArrowLeft/ArrowRight page a PDF viewer open in the inactive pane (no pane switch needed)', () => {
    const nextPage = vi.fn();
    const previousPage = vi.fn();
    const context = makeContext({
      getViewer: (paneId) =>
        paneId === PANE_B
          ? ({
              controller: { nextPage, previousPage } as never,
              state: { status: 'ready', content: { kind: 'pdf' } } as never,
            } as never)
          : undefined,
    });

    createGlobalKeydownHandler(context)(keydown('ArrowRight'));
    createGlobalKeydownHandler(context)(keydown('ArrowLeft'));
    expect(nextPage).toHaveBeenCalledTimes(1);
    expect(previousPage).toHaveBeenCalledTimes(1);
  });

  it('ArrowLeft/ArrowRight page a PPTX content preview', () => {
    const nextPage = vi.fn();
    const previousPage = vi.fn();
    const context = makeContext({
      getViewer: (paneId) =>
        paneId === PANE_B
          ? ({
              controller: { nextPage, previousPage } as never,
              state: { status: 'ready', content: { kind: 'pptx' } } as never,
            } as never)
          : undefined,
    });
    createGlobalKeydownHandler(context)(keydown('ArrowRight'));
    createGlobalKeydownHandler(context)(keydown('ArrowLeft'));
    expect(nextPage).toHaveBeenCalledTimes(1);
    expect(previousPage).toHaveBeenCalledTimes(1);
  });

  it('does not intercept ArrowLeft/ArrowRight when no viewer is open (or it is showing non-paged content)', () => {
    const nextPage = vi.fn();
    const context = makeContext({
      getViewer: (paneId) =>
        paneId === PANE_B
          ? ({
              controller: { nextPage } as never,
              state: { status: 'ready', content: { kind: 'text' } } as never,
            } as never)
          : undefined,
    });
    const event = keydown('ArrowRight');
    const preventDefault = vi.spyOn(event, 'preventDefault');
    createGlobalKeydownHandler(context)(event);
    expect(nextPage).not.toHaveBeenCalled();
    expect(preventDefault).not.toHaveBeenCalled();
  });

  it('ArrowUp/Down and PageUp/Down scroll/page a text viewer once focus is inside it', () => {
    const scrollViewer = vi.fn();
    const context = makeContext({
      getViewer: (paneId) =>
        paneId === PANE_B
          ? ({
              controller: {} as never,
              state: { status: 'ready', content: { kind: 'text' } } as never,
            } as never)
          : undefined,
      scrollViewer,
    });
    createGlobalKeydownHandler(context)(viewerKeydown('ArrowDown'));
    createGlobalKeydownHandler(context)(viewerKeydown('ArrowUp'));
    createGlobalKeydownHandler(context)(viewerKeydown('PageDown'));
    createGlobalKeydownHandler(context)(viewerKeydown('PageUp'));
    expect(scrollViewer).toHaveBeenNthCalledWith(1, PANE_B, 0, 1, 'line');
    expect(scrollViewer).toHaveBeenNthCalledWith(2, PANE_B, 0, -1, 'line');
    expect(scrollViewer).toHaveBeenNthCalledWith(3, PANE_B, 0, 1, 'page');
    expect(scrollViewer).toHaveBeenNthCalledWith(4, PANE_B, 0, -1, 'page');
  });

  it('does not scroll a text viewer from Arrow/Page keys outside the viewer (would otherwise fight the directory table cursor)', () => {
    const scrollViewer = vi.fn();
    const context = makeContext({
      getViewer: (paneId) =>
        paneId === PANE_B
          ? ({
              controller: {} as never,
              state: { status: 'ready', content: { kind: 'text' } } as never,
            } as never)
          : undefined,
      scrollViewer,
    });
    createGlobalKeydownHandler(context)(keydown('ArrowDown'));
    expect(scrollViewer).not.toHaveBeenCalled();
  });

  it('still scrolls a text viewer when focus is in its own search input or read-only CodeMirror body', () => {
    const scrollViewer = vi.fn();
    const context = makeContext({
      getViewer: (paneId) =>
        paneId === PANE_B
          ? ({
              controller: {} as never,
              state: { status: 'ready', content: { kind: 'text' } } as never,
            } as never)
          : undefined,
      scrollViewer,
    });
    createGlobalKeydownHandler(context)(
      viewerKeydown('ArrowDown', {}, (section) => {
        const input = document.createElement('input');
        input.className = 'fm-file-viewer-search-input';
        section.append(input);
        return input;
      }),
    );
    createGlobalKeydownHandler(context)(
      viewerKeydown('ArrowDown', {}, (section) => {
        const editor = document.createElement('div');
        editor.className = 'cm-editor';
        const content = document.createElement('div');
        content.className = 'cm-content';
        content.contentEditable = 'true';
        editor.append(content);
        section.append(editor);
        return content;
      }),
    );
    expect(scrollViewer).toHaveBeenCalledTimes(2);
  });

  it('does not hijack Arrow keys from an unrelated editable field inside the viewer (e.g. renaming a tab)', () => {
    const scrollViewer = vi.fn();
    const context = makeContext({
      getViewer: (paneId) =>
        paneId === PANE_B
          ? ({
              controller: {} as never,
              state: { status: 'ready', content: { kind: 'text' } } as never,
            } as never)
          : undefined,
      scrollViewer,
    });
    createGlobalKeydownHandler(context)(
      viewerKeydown('ArrowDown', {}, (section) => {
        const renameInput = document.createElement('input');
        renameInput.className = 'fm-tab-rename-input';
        section.append(renameInput);
        return renameInput;
      }),
    );
    expect(scrollViewer).not.toHaveBeenCalled();
  });

  it('Arrow keys pan an image viewer once focus is inside it', () => {
    const scrollViewer = vi.fn();
    const context = makeContext({
      getViewer: (paneId) =>
        paneId === PANE_B
          ? ({
              controller: {} as never,
              state: { status: 'ready', content: { kind: 'image' } } as never,
            } as never)
          : undefined,
      scrollViewer,
    });
    createGlobalKeydownHandler(context)(viewerKeydown('ArrowUp'));
    createGlobalKeydownHandler(context)(viewerKeydown('ArrowDown'));
    createGlobalKeydownHandler(context)(viewerKeydown('ArrowLeft'));
    createGlobalKeydownHandler(context)(viewerKeydown('ArrowRight'));
    expect(scrollViewer).toHaveBeenNthCalledWith(1, PANE_B, 0, -1, 'line');
    expect(scrollViewer).toHaveBeenNthCalledWith(2, PANE_B, 0, 1, 'line');
    expect(scrollViewer).toHaveBeenNthCalledWith(3, PANE_B, -1, 0, 'line');
    expect(scrollViewer).toHaveBeenNthCalledWith(4, PANE_B, 1, 0, 'line');
  });

  it('PageUp/PageDown and +/- zoom an image viewer once focus is inside it', () => {
    const zoomIn = vi.fn();
    const zoomOut = vi.fn();
    const context = makeContext({
      getViewer: (paneId) =>
        paneId === PANE_B
          ? ({
              controller: { zoomIn, zoomOut } as never,
              state: { status: 'ready', content: { kind: 'image' } } as never,
            } as never)
          : undefined,
    });
    createGlobalKeydownHandler(context)(viewerKeydown('PageUp'));
    createGlobalKeydownHandler(context)(viewerKeydown('PageDown'));
    createGlobalKeydownHandler(context)(viewerKeydown('+'));
    createGlobalKeydownHandler(context)(viewerKeydown('-'));
    expect(zoomIn).toHaveBeenCalledTimes(2);
    expect(zoomOut).toHaveBeenCalledTimes(2);
  });

  it('F3/Shift+F3 navigate search matches once focus is inside a text viewer', () => {
    const goToNextMatch = vi.fn();
    const goToPreviousMatch = vi.fn();
    const context = makeContext({
      getViewer: (paneId) =>
        paneId === PANE_B
          ? ({
              controller: { goToNextMatch, goToPreviousMatch } as never,
              state: { status: 'ready', content: { kind: 'text' } } as never,
            } as never)
          : undefined,
    });
    createGlobalKeydownHandler(context)(viewerKeydown('F3'));
    createGlobalKeydownHandler(context)(viewerKeydown('F3', { shiftKey: true }));
    expect(goToNextMatch).toHaveBeenCalledTimes(1);
    expect(goToPreviousMatch).toHaveBeenCalledTimes(1);
  });

  it('does not intercept F3 for search navigation outside the viewer, or for non-text content', () => {
    const goToNextMatch = vi.fn();
    const context = makeContext({
      getViewer: (paneId) =>
        paneId === PANE_B
          ? ({
              controller: { goToNextMatch } as never,
              state: { status: 'ready', content: { kind: 'text' } } as never,
            } as never)
          : undefined,
    });
    createGlobalKeydownHandler(context)(keydown('F3'));
    expect(goToNextMatch).not.toHaveBeenCalled();
  });

  it('Alt+Space opens a viewer (with the info panel shown) for the cursor file when none is open yet', () => {
    const openViewer = vi.fn();
    const cursorFile: EntrySummary = {
      id: 'file-1' as never,
      location: { providerId: 'local', uri: 'file:///a/b/report.txt' },
      name: 'report.txt',
      kind: 'file',
      hidden: false,
      readOnly: false,
      metadataRevision: 0,
    };
    const context = makeContext({
      getViewer: () => undefined,
      openViewer,
      getSelections: () =>
        new Map([['pane-a:tab', { selectedEntryIds: [], cursorEntryId: 'file-1' as never }]]),
      getDirectories: () =>
        new Map([['pane-a:tab', { entries: [cursorFile] } as unknown as PaneDirectoryView]]),
    });
    const event = keydown(' ', { altKey: true, code: 'Space' });
    const preventDefault = vi.spyOn(event, 'preventDefault');
    createGlobalKeydownHandler(context)(event);
    expect(preventDefault).toHaveBeenCalled();
    expect(openViewer).toHaveBeenCalledWith(PANE_B, cursorFile, undefined, true);
  });

  it('Alt+Space does nothing when no viewer is open', () => {
    const context = makeContext({ getViewer: () => undefined });
    const event = keydown(' ', { altKey: true, code: 'Space' });
    const preventDefault = vi.spyOn(event, 'preventDefault');
    createGlobalKeydownHandler(context)(event);
    expect(preventDefault).not.toHaveBeenCalled();
  });

  it('does not dispatch pane switching when Tab originates in a modal dialog', () => {
    const focusPane = vi.fn();
    const context = makeContext({
      focusPane,
      actionsWithFavourites: () => [
        ...ACTIONS,
        {
          id: 'core.switchPane',
          title: 'Switch pane',
          category: 'navigation',
          defaultShortcuts: [{ key: 'Tab' }],
          contextRequirements: {},
          source: { kind: 'core' },
        },
      ],
    });
    const dialog = document.createElement('div');
    dialog.setAttribute('role', 'dialog');
    const button = document.createElement('button');
    dialog.append(button);
    document.body.append(dialog);
    const event = keydown('Tab');
    const handler = createGlobalKeydownHandler(context);
    document.addEventListener('keydown', handler);
    button.dispatchEvent(event);
    document.removeEventListener('keydown', handler);
    dialog.remove();

    expect(focusPane).not.toHaveBeenCalled();
    expect(event.defaultPrevented).toBe(false);
  });

  it('keeps bare F8 on Trash and Shift+F8 on permanent delete when Trash is available', () => {
    const trash = vi.fn().mockResolvedValue({});
    const deletePermanently = vi.fn().mockResolvedValue({});
    const cursorFile: EntrySummary = {
      id: 'file-1' as never,
      location: { providerId: 'local', uri: 'file:///a/report.txt' },
      name: 'report.txt',
      kind: 'file',
      hidden: false,
      readOnly: false,
      metadataRevision: 0,
    };
    const actions: readonly ActionDescriptor[] = [
      {
        id: 'core.delete',
        title: 'Delete',
        category: 'fileOperations',
        defaultShortcuts: [{ key: 'F8' }],
        contextRequirements: {},
        source: { kind: 'core' },
      },
      {
        id: 'core.trash',
        title: 'Trash',
        category: 'fileOperations',
        defaultShortcuts: [{ key: 'F8' }],
        contextRequirements: {},
        source: { kind: 'core' },
      },
    ];
    const context = makeContext({
      actionsWithFavourites: () => actions,
      getRegisteredActions: () => actions,
      getOpsController: () =>
        ({ trash, delete: deletePermanently }) as unknown as OperationsController,
      getSelections: () =>
        new Map([['pane-a:tab', { selectedEntryIds: [], cursorEntryId: 'file-1' as never }]]),
      getDirectories: () =>
        new Map([['pane-a:tab', { entries: [cursorFile] } as unknown as PaneDirectoryView]]),
    });
    const handler = createGlobalKeydownHandler(context);
    handler(keydown('F8'));
    handler(keydown('F8', { shiftKey: true }));

    expect(trash).toHaveBeenCalledOnce();
    expect(deletePermanently).toHaveBeenCalledOnce();
  });

  // task 0134: "Selected thumbnails can use F3 to see the full screen version" - F3's viewer
  // resolution (resolveViewTarget/openViewer) reads only the cursor entry from
  // getSelections/getDirectories, which carry no view-mode concept at all, so the same F3 handling
  // already used for the table applies unchanged whether the active pane is showing its listing as
  // a table or as a thumbnail grid - there is nothing grid-specific to wire up.
  it('F3 opens the viewer for the cursor entry regardless of the active pane’s view mode', () => {
    const openViewer = vi.fn();
    const cursorFile: EntrySummary = {
      id: 'photo-1' as never,
      location: { providerId: 'local', uri: 'file:///a/b/photo.png' },
      name: 'photo.png',
      kind: 'file',
      hidden: false,
      readOnly: false,
      metadataRevision: 0,
    };
    const coreViewAction: ActionDescriptor = {
      id: 'core.view',
      title: 'View',
      defaultShortcuts: [{ key: 'F3' }],
      category: 'test',
      contextRequirements: {},
      source: { kind: 'core' },
    };
    const context = makeContext({
      getRegisteredActions: () => [...ACTIONS, coreViewAction],
      actionsWithFavourites: () => [...ACTIONS, coreViewAction],
      getViewer: () => undefined,
      openViewer,
      // A grid-mode pane's directory/selection state shapes are identical to a table-mode pane's -
      // there is no `viewMode` field here to gate on, which is exactly the point.
      getSelections: () =>
        new Map([['pane-a:tab', { selectedEntryIds: [], cursorEntryId: 'photo-1' as never }]]),
      getDirectories: () =>
        new Map([['pane-a:tab', { entries: [cursorFile] } as unknown as PaneDirectoryView]]),
    });
    createGlobalKeydownHandler(context)(keydown('F3'));
    expect(openViewer).toHaveBeenCalledWith(PANE_B, cursorFile, undefined);
  });
});
