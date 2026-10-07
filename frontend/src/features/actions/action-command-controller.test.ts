import { describe, expect, it, vi } from 'vitest';
import type { ActionDescriptor, EntryId, EntrySummary, PaneId } from '../../models';
import type { PaneDirectoryView } from '../navigation/navigation';
import { withParentEntry } from '../panes/parent-entry';
import {
  type ActionCommandControllerContext,
  createActionCommandController,
} from './action-command-controller';

function uninstallAction(): ActionDescriptor {
  return {
    id: 'core.uninstallApplication',
    title: 'Uninstall Application…',
    category: 'fileOperations',
    defaultShortcuts: [],
    contextRequirements: { featureAvailable: true, requiresSingleSelection: true },
    source: { kind: 'core' },
  };
}

function documentSummaryAction(): ActionDescriptor {
  return {
    id: 'core.documentSummary',
    title: 'Summarize document…',
    category: 'fileOperations',
    defaultShortcuts: [],
    contextRequirements: { featureAvailable: true, requiresSingleSelection: true },
    source: { kind: 'core' },
  };
}

function bundleEntry(): EntrySummary {
  return {
    id: 'widget-app' as EntryId,
    location: { providerId: 'local', uri: 'file:///Applications/Widget.app' },
    name: 'Widget.app',
    kind: 'file',
    hidden: false,
    readOnly: false,
    metadataRevision: 1,
  };
}

/** Minimal fake satisfying every context member; only what a given test cares about is configured. */
function fakeContext(
  overrides: Partial<ActionCommandControllerContext> = {},
): ActionCommandControllerContext {
  return {
    getCommandPaletteOpen: () => false,
    setCommandPaletteOpen: () => {},
    getContextMenu: () => undefined,
    setContextMenu: () => {},
    getCommandPaletteRecency: () => new Map(),
    getActiveDirectory: () => undefined,
    getActiveTabKey: (paneId) => paneId,
    getSelections: () => new Map(),
    getDirectories: () => new Map(),
    getCurrentSettings: () => undefined,
    getClient: () => {
      throw new Error('not needed for this test');
    },
    openPluginPane: () => 'opened',
    getRegisteredActions: () => [],
    getPlugins: () => [],
    getWorkspace: () => undefined,
    getNavigation: () => {
      throw new Error('not needed for this test');
    },
    getOpsController: () => {
      throw new Error('not needed for this test');
    },
    getGetSelectedEntries: () => () => [],
    getClipboard: () => ({ locations: [] }),
    replaceClipboard: () => {},
    toast: () => {},
    getOpenTerminalSupported: () => false,
    openCreateDirectory: () => {},
    setArchiveCreateRequest: () => {},
    openFinderTagsDialog: () => {},
    openSpotlightCommentDialog: () => {},
    calculateChecksums: () => {},
    findDuplicates: () => {},
    openDiskUsage: () => {},
    openPropertiesForActivePane: () => {},
    openMultiRenameForActivePane: () => {},
    openSemanticAssistant: () => {},
    openKnowledgeSearch: () => {},
    uninstallApplication: () => {},
    toggleDirectoryTree: () => {},
    toggleOperationCentre: () => {},
    redraw: () => {},
    ...overrides,
  };
}

describe('SPA panel cursor activation', () => {
  it('opens only the SVG under the cursor even when another file is marked', async () => {
    const paneId = 'pane-1';
    const marked = bundleEntry();
    const svg: EntrySummary = {
      ...marked,
      id: 'drawing' as EntryId,
      name: 'drawing.SVG',
      location: { providerId: 'local', uri: 'file:///drawing.SVG' },
    };
    const openPluginPane = vi.fn().mockReturnValue('opened');
    const toast = vi.fn();
    const context = fakeContext({
      getActiveDirectory: () => ({ paneId, location: { providerId: 'local', uri: 'file:///' } }),
      getSelections: () =>
        new Map([[paneId, { selectedEntryIds: [marked.id], cursorEntryId: svg.id }]]),
      getDirectories: () =>
        new Map([[paneId, { state: { type: 'loaded' }, entries: [marked, svg], hasMore: false }]]),
      getPlugins: () => [
        {
          id: 'example.svgo',
          name: 'SVGO',
          version: '1',
          description: '',
          enabled: true,
          spaPanel: { actionId: 'example.svgo.open', extensions: ['svg'] },
        },
      ],
      openPluginPane,
      toast,
    });
    const controller = createActionCommandController(context);
    const invocation = {
      paneId,
      cursorEntryId: svg.id,
      selectedEntryIds: [marked.id],
    };
    controller.invokeActionById('example.svgo.open', undefined, invocation);
    expect(openPluginPane).toHaveBeenCalledWith(
      paneId,
      expect.objectContaining({ id: 'example.svgo' }),
      'example.svgo.open',
      svg,
    );
    expect(toast).not.toHaveBeenCalled();

    openPluginPane.mockReturnValueOnce('unavailable');
    controller.invokeActionById('example.svgo.open', undefined, invocation);
    expect(toast).toHaveBeenCalledWith({ html: 'Unable to run command.' });
  });

  it('does not open a panel for a non-SVG cursor', () => {
    const entry = bundleEntry();
    const openPluginPane = vi.fn();
    const context = fakeContext({
      getActiveDirectory: () => ({
        paneId: 'pane-1',
        location: { providerId: 'local', uri: 'file:///' },
      }),
      getSelections: () => new Map([['pane-1', { selectedEntryIds: [], cursorEntryId: entry.id }]]),
      getDirectories: () =>
        new Map([['pane-1', { state: { type: 'loaded' }, entries: [entry], hasMore: false }]]),
      getPlugins: () => [
        {
          id: 'example.svgo',
          name: 'SVGO',
          version: '1',
          description: '',
          enabled: true,
          spaPanel: { actionId: 'example.svgo.open', extensions: ['svg'] },
        },
      ],
      openPluginPane,
    });
    createActionCommandController(context).invokeActionById('example.svgo.open', undefined, {
      paneId: 'pane-1',
      cursorEntryId: entry.id,
    });
    expect(openPluginPane).not.toHaveBeenCalled();
  });

  it('does not open SVGO for a remote SVG even through direct action invocation', () => {
    const svg = {
      ...bundleEntry(),
      id: 'remote-svg' as EntryId,
      name: 'drawing.svg',
      location: { providerId: 'sftp', uri: 'sftp://host/drawing.svg' },
    };
    const openPluginPane = vi.fn();
    const toast = vi.fn();
    const context = fakeContext({
      getActiveDirectory: () => ({
        paneId: 'pane-1',
        location: { providerId: 'sftp', uri: 'sftp://host/' },
      }),
      getSelections: () => new Map([['pane-1', { selectedEntryIds: [], cursorEntryId: svg.id }]]),
      getDirectories: () =>
        new Map([['pane-1', { state: { type: 'loaded' }, entries: [svg], hasMore: false }]]),
      getPlugins: () => [
        {
          id: 'procyon.svgo',
          name: 'SVGO',
          version: '1',
          description: '',
          enabled: true,
          spaPanel: { actionId: 'procyon.svgo.open', extensions: ['svg'] },
        },
      ],
      openPluginPane,
      toast,
    });
    createActionCommandController(context).invokeActionById('procyon.svgo.open', undefined, {
      paneId: 'pane-1',
      cursorEntryId: svg.id,
    });
    expect(openPluginPane).not.toHaveBeenCalled();
    expect(toast).toHaveBeenCalledWith({ html: 'SVGO can edit only local SVG files' });
  });
});

describe('action-command-controller uninstallApplication wiring', () => {
  it('opens the document-summary flow without invoking the synchronous backend action', () => {
    const paneId = 'pane-1' as PaneId;
    const file = bundleEntry();
    const directory: PaneDirectoryView = {
      state: { type: 'loaded' },
      entries: [file],
      hasMore: false,
    };
    const openDocumentSummary = vi.fn();
    const context = fakeContext({
      getRegisteredActions: () => [documentSummaryAction()],
      getDirectories: () => new Map([[paneId, directory]]),
      getActiveTabKey: () => paneId,
      openDocumentSummary,
    });

    createActionCommandController(context).invokePaletteAction(documentSummaryAction(), undefined, {
      paneId,
      selectedEntryIds: [file.id],
    });

    expect(openDocumentSummary).toHaveBeenCalledWith(paneId, file);
  });

  it('dispatches client.searchKnowledge to the knowledge search dialog (task 0206)', () => {
    const openKnowledgeSearch = vi.fn();
    const getClient = vi.fn();
    const searchKnowledge = {
      id: 'client.searchKnowledge',
      title: 'Search Knowledge…',
      category: 'tools',
      defaultShortcuts: [{ key: 'k', ctrl: true, shift: true }],
      contextRequirements: {},
      source: { kind: 'core' as const },
    };
    const context = fakeContext({
      getRegisteredActions: () => [searchKnowledge],
      openKnowledgeSearch,
      getClient,
    });

    createActionCommandController(context).invokePaletteAction(searchKnowledge, undefined, {});

    expect(openKnowledgeSearch).toHaveBeenCalledOnce();
    expect(getClient).not.toHaveBeenCalled();
  });

  it('opens the Multi-Rename Tool from the palette instead of the generic backend invoke', () => {
    const openMultiRenameForActivePane = vi.fn();
    const getClient = vi.fn();
    const multiRename = {
      id: 'core.openMultiRename',
      title: 'Multi-Rename Tool',
      category: 'file',
      defaultShortcuts: [],
      contextRequirements: {},
      source: { kind: 'core' as const },
    };
    const context = fakeContext({ openMultiRenameForActivePane, getClient });

    createActionCommandController(context).invokePaletteAction(multiRename, undefined, {});

    expect(openMultiRenameForActivePane).toHaveBeenCalledOnce();
    expect(getClient).not.toHaveBeenCalled();
  });

  it('invokePaletteAction dispatches the real discovery flow instead of the generic backend invoke', () => {
    const paneId = 'pane-1' as PaneId;
    const bundle = bundleEntry();
    const directory: PaneDirectoryView = {
      state: { type: 'loaded' },
      entries: [bundle],
      hasMore: false,
    };
    const uninstallApplication = vi.fn();
    const context = fakeContext({
      getRegisteredActions: () => [uninstallAction()],
      getDirectories: () => new Map([[paneId, directory]]),
      getActiveTabKey: () => paneId,
      uninstallApplication,
    });
    const controller = createActionCommandController(context);

    controller.invokePaletteAction(uninstallAction(), undefined, {
      paneId,
      selectedEntryIds: [bundle.id],
    });

    expect(uninstallApplication).toHaveBeenCalledWith(paneId, bundle);
  });

  it('invokeContextMenuAction dispatches the real discovery flow for the right-click menu', () => {
    const paneId = 'pane-1' as PaneId;
    const bundle = bundleEntry();
    const directory: PaneDirectoryView = {
      state: { type: 'loaded' },
      entries: [bundle],
      hasMore: false,
    };
    const uninstallApplication = vi.fn();
    const context = fakeContext({
      getRegisteredActions: () => [uninstallAction()],
      getDirectories: () => new Map([[paneId, directory]]),
      getActiveTabKey: () => paneId,
      getContextMenu: () => ({ paneId, entries: [bundle], x: 0, y: 0 }),
      uninstallApplication,
    });
    const controller = createActionCommandController(context);

    controller.invokeContextMenuAction('core.uninstallApplication');

    expect(uninstallApplication).toHaveBeenCalledWith(paneId, bundle);
  });

  it('invokeContextMenuAction does nothing when the action is unavailable (e.g. multi-selection)', () => {
    const paneId = 'pane-1' as PaneId;
    const bundle = bundleEntry();
    const other: EntrySummary = { ...bundle, id: 'other' as EntryId, name: 'Other.app' };
    const directory: PaneDirectoryView = {
      state: { type: 'loaded' },
      entries: [bundle, other],
      hasMore: false,
    };
    const uninstallApplication = vi.fn();
    const context = fakeContext({
      getRegisteredActions: () => [uninstallAction()],
      getDirectories: () => new Map([[paneId, directory]]),
      getActiveTabKey: () => paneId,
      getContextMenu: () => ({ paneId, entries: [bundle, other], x: 0, y: 0 }),
      uninstallApplication,
    });
    const controller = createActionCommandController(context);

    controller.invokeContextMenuAction('core.uninstallApplication');

    expect(uninstallApplication).not.toHaveBeenCalled();
  });
});

function fileOperationAction(id: string): ActionDescriptor {
  return {
    id,
    title: id,
    category: 'fileOperations',
    defaultShortcuts: [],
    contextRequirements: {},
    source: { kind: 'core' },
  };
}

function folderEntry(providerId = 'local'): EntrySummary {
  return {
    id: 'semantic' as EntryId,
    location: {
      providerId,
      uri: `${providerId === 'local' ? 'file' : providerId}:///work/semantic`,
    },
    name: 'semantic',
    kind: 'directory',
    hidden: false,
    readOnly: false,
    metadataRevision: 1,
  };
}

describe('action-command-controller file operations from the context menu and palette', () => {
  const paneId = 'pane-1' as PaneId;
  const otherPaneId = 'pane-2' as PaneId;
  const destination = { providerId: 'local', uri: 'file:///elsewhere' };

  function setup(actionId: string, entry = folderEntry()) {
    const directory: PaneDirectoryView = {
      state: { type: 'loaded' },
      entries: [entry],
      hasMore: false,
    };
    const otherDirectory: PaneDirectoryView = {
      state: { type: 'loaded' },
      location: destination,
      entries: [],
      hasMore: false,
    };
    const ops = { delete: vi.fn(), trash: vi.fn(), copy: vi.fn(), move: vi.fn() };
    const getClient = vi.fn();
    const context = fakeContext({
      getRegisteredActions: () => [fileOperationAction(actionId)],
      getDirectories: () =>
        new Map([
          [paneId, directory],
          [otherPaneId, otherDirectory],
        ]),
      getWorkspace: () =>
        ({ paneOrder: [paneId, otherPaneId] }) as unknown as ReturnType<
          ActionCommandControllerContext['getWorkspace']
        >,
      getContextMenu: () => ({ paneId, entries: [entry], x: 0, y: 0 }),
      getOpsController: () =>
        ops as unknown as ReturnType<ActionCommandControllerContext['getOpsController']>,
      getClient,
    });
    return { controller: createActionCommandController(context), ops, getClient, entry };
  }

  it('deletes a folder through the confirmed operation flow instead of the parameterless backend invoke', () => {
    const { controller, ops, getClient, entry } = setup('core.delete');

    controller.invokeContextMenuAction('core.delete');

    expect(ops.delete).toHaveBeenCalledWith([entry.location], false, false);
    expect(getClient).not.toHaveBeenCalled();
  });

  it('trashes local entries and falls back to a confirmed delete where no system trash exists', () => {
    const local = setup('core.trash');
    local.controller.invokeContextMenuAction('core.trash');
    expect(local.ops.trash).toHaveBeenCalledWith([local.entry.location]);

    const remote = setup('core.trash', folderEntry('sftp'));
    remote.controller.invokeContextMenuAction('core.trash');
    expect(remote.ops.trash).not.toHaveBeenCalled();
    expect(remote.ops.delete).toHaveBeenCalledWith([remote.entry.location], false, false);
  });

  it('copies and moves to the other pane', () => {
    const copy = setup('core.copy');
    copy.controller.invokeContextMenuAction('core.copy');
    expect(copy.ops.copy).toHaveBeenCalledWith([copy.entry.location], destination);

    const move = setup('core.move');
    move.controller.invokeContextMenuAction('core.move');
    expect(move.ops.move).toHaveBeenCalledWith([move.entry.location], destination);
    expect(move.getClient).not.toHaveBeenCalled();
  });

  it('uses the cursor entry when the palette runs delete without a selection', () => {
    const { controller, ops, entry } = setup('core.delete');

    controller.invokePaletteAction(fileOperationAction('core.delete'), undefined, {
      paneId,
      cursorEntryId: entry.id,
    });

    expect(ops.delete).toHaveBeenCalledWith([entry.location], false, false);
  });

  it('asks the pane to start its F2 rename flow instead of invoking the backend', () => {
    const entry = folderEntry();
    const requestRename = vi.fn();
    const getClient = vi.fn();
    const directory: PaneDirectoryView = {
      state: { type: 'loaded' },
      entries: [entry],
      hasMore: false,
    };
    const controller = createActionCommandController(
      fakeContext({
        getRegisteredActions: () => [fileOperationAction('core.rename')],
        getDirectories: () => new Map([[paneId, directory]]),
        getContextMenu: () => ({ paneId, entries: [entry], x: 0, y: 0 }),
        requestRename,
        getClient,
      }),
    );

    controller.invokeContextMenuAction('core.rename');

    expect(requestRename).toHaveBeenCalledWith(paneId, [entry.id]);
    expect(getClient).not.toHaveBeenCalled();
  });
});

describe('action-command-controller context-menu copy actions', () => {
  it('copies the parent folder path for the synthetic ".." entry', async () => {
    const paneId = 'pane-1' as PaneId;
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal('navigator', { clipboard: { writeText } });
    const parent = withParentEntry('/Users/me/Downloads', [bundleEntry()])[0];
    if (parent === undefined) throw new Error('expected a parent entry');
    const copyPath = {
      id: 'core.copyPath',
      title: 'Copy Full Path',
      category: 'edit',
      defaultShortcuts: [],
      contextRequirements: {},
      source: { kind: 'core' as const },
    };
    const context = fakeContext({
      getRegisteredActions: () => [copyPath],
      getContextMenu: () => ({ paneId, entries: [parent], x: 0, y: 0 }),
      getDirectories: () =>
        new Map([
          [
            paneId,
            {
              state: { type: 'loaded' },
              location: { providerId: 'local', uri: 'file:///Users/me/Downloads' },
              entries: [bundleEntry()],
              hasMore: false,
            } as PaneDirectoryView,
          ],
        ]),
    });

    createActionCommandController(context).invokeContextMenuAction('core.copyPath');
    await vi.waitFor(() => expect(writeText).toHaveBeenCalledWith('/Users/me'));
    vi.unstubAllGlobals();
  });
});
