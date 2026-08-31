import { describe, expect, it, vi } from 'vitest';

import type { ActionDescriptor, EntryId, EntrySummary, PaneId } from '../../models';
import type { PaneDirectoryView } from '../navigation/navigation';
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
    getRegisteredActions: () => [],
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
    uninstallApplication: () => {},
    toggleDirectoryTree: () => {},
    redraw: () => {},
    ...overrides,
  };
}

describe('action-command-controller uninstallApplication wiring', () => {
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
