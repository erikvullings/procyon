import m from 'mithril';
import {
  ancestorChain,
  createTreeChildrenState,
  flattenVisibleTree,
  withChildren,
  withError,
  withExpanded,
  withLoading,
} from '../features/directory-tree/directory-tree-state';
import type { DropModifiers } from '../features/drag-drop/drag-drop';
import { operationForDrop, validateDropTarget } from '../features/drag-drop/drag-drop';
import { parentLocation } from '../features/navigation/navigation';
import { rootLocationFor } from '../features/navigation/root-location';
import { t } from '../i18n';
import type { Location } from '../models';
import { treeRootName } from './shell-helpers';
/** Sets up the treeWiring feature wiring and returns its functions. */
import type { ShellState } from './shell-state';

/** The narrow cross-cutting surface directory-tree wiring needs from the shell. */
export interface TreeCore {
  readonly activeTabKey: (paneId: import('../models').PaneId) => string;
  readonly activeDirectory: () =>
    | {
        readonly paneId: import('../models').PaneId;
        readonly location: import('../models').Location;
      }
    | undefined;
}

export function createTreeWiring(shellState: ShellState, core: TreeCore) {
  function activeTerminalTabKey(): string | undefined {
    const active = core.activeDirectory();
    return active === undefined ? undefined : core.activeTabKey(active.paneId);
  }

  function ensureTreeChildrenLoaded(location: Location): void {
    if (
      location.uri in shellState.treeState.childrenByUri ||
      shellState.treeState.loadingUris.has(location.uri)
    )
      return;
    const generation = shellState.treeLoadGeneration;
    shellState.treeState = withLoading(shellState.treeState, location.uri, true);
    shellState.client
      .listDirectoryChildren(location, false)
      .then((children) => {
        if (generation !== shellState.treeLoadGeneration) return;
        shellState.treeState = withChildren(shellState.treeState, location.uri, children);
        m.redraw();
      })
      .catch((error: unknown) => {
        if (generation !== shellState.treeLoadGeneration) return;
        shellState.treeState = withError(
          shellState.treeState,
          location.uri,
          error instanceof Error ? error.message : t('tree', 'unableToLoad'),
        );
        m.redraw();
      });
  }

  function refreshDirectoryTree(): void {
    if (!shellState.treeSidebarOpen) {
      shellState.treeNeedsRefresh = true;
      return;
    }
    if (shellState.treeRootLocation === undefined) return;
    shellState.treeNeedsRefresh = false;
    const visible = flattenVisibleTree(
      { location: shellState.treeRootLocation, name: treeRootName(shellState.treeRootLocation) },
      shellState.treeState,
    );
    shellState.treeLoadGeneration += 1;
    shellState.treeState = {
      ...shellState.treeState,
      childrenByUri: {},
      loadingUris: new Set(),
      errorByUri: {},
    };
    for (const row of visible) {
      if (row.expanded) ensureTreeChildrenLoaded(row.location);
    }
  }

  function treeDropWritable(location: Location): boolean {
    if (
      location.providerId === 'archive' ||
      shellState.unavailableLocations.has(`${location.providerId}:${location.uri}`) ||
      shellState.systemLocations.some(
        (system) =>
          system.readOnly === true &&
          system.location.providerId === location.providerId &&
          (location.uri === system.location.uri ||
            location.uri.startsWith(
              system.location.uri.endsWith('/') ? system.location.uri : `${system.location.uri}/`,
            )),
      )
    )
      return false;
    const entry = shellState.treeState.childrenByUri[parentLocation(location).uri]?.find(
      (child) =>
        child.location.uri === location.uri && child.location.providerId === location.providerId,
    );
    if (entry?.readOnly === true) return false;
    const loaded = [...shellState.directories.values()].find(
      (directory) =>
        directory.location?.uri === location.uri &&
        directory.location.providerId === location.providerId,
    );
    return loaded?.writable !== false;
  }

  function dropOnTree(location: Location, modifiers: DropModifiers): void {
    const validation = validateDropTarget(
      shellState.draggedLocations,
      location,
      treeDropWritable(location),
    );
    if (!validation.ok) {
      if (!validation.noOp) shellState.clipboardMessage = validation.message;
      return;
    }
    const sources = shellState.draggedLocations;
    shellState.draggedLocations = [];
    void (shellState.nativeDropInProgress ||
    operationForDrop(shellState.platform, modifiers) === 'copy'
      ? shellState.opsController.copy(sources, location)
      : shellState.opsController.move(sources, location));
  }

  function toggleTreeNode(location: Location): void {
    if (shellState.treeState.expanded.has(location.uri)) {
      shellState.treeState = withExpanded(shellState.treeState, location.uri, false);
      return;
    }
    if (location.uri in shellState.treeState.childrenByUri) {
      shellState.treeState = withExpanded(shellState.treeState, location.uri, true);
      return;
    }
    ensureTreeChildrenLoaded(location);
  }

  function syncDirectoryTreeToActiveLocation(): void {
    const active = core.activeDirectory();
    if (active === undefined || active.location.uri === shellState.treeSyncedLocationUri) return;
    shellState.treeSyncedLocationUri = active.location.uri;
    const root = rootLocationFor(active.location);
    if (shellState.treeRootLocation === undefined || shellState.treeRootLocation.uri !== root.uri) {
      shellState.treeLoadGeneration += 1;
      shellState.treeNeedsRefresh = false;
      shellState.treeRootLocation = root;
      shellState.treeState = createTreeChildrenState();
    }
    // `ancestorChain` excludes `root` itself (a direct child of root yields an empty chain), but
    // the root row still must be expanded for that child to appear at all - so expand+load it
    // explicitly whenever the active location is anywhere below it.
    if (active.location.uri !== root.uri) {
      shellState.treeState = withExpanded(shellState.treeState, root.uri, true);
      ensureTreeChildrenLoaded(root);
    }
    for (const ancestor of ancestorChain(root, active.location)) {
      shellState.treeState = withExpanded(shellState.treeState, ancestor.uri, true);
      ensureTreeChildrenLoaded(ancestor);
    }
  }

  function toggleDirectoryTree(): void {
    shellState.treeSidebarOpen = !shellState.treeSidebarOpen;
    if (shellState.treeSidebarOpen) {
      syncDirectoryTreeToActiveLocation();
      if (shellState.treeNeedsRefresh) refreshDirectoryTree();
      requestAnimationFrame(() => shellState.focusDirectoryTree?.());
    } else {
      requestAnimationFrame(() => {
        const activePaneId = shellState.workspace?.activePaneId;
        if (activePaneId !== undefined) shellState.focusPane?.(activePaneId);
      });
    }
  }

  return {
    activeTerminalTabKey,
    ensureTreeChildrenLoaded,
    refreshDirectoryTree,
    treeDropWritable,
    dropOnTree,
    toggleTreeNode,
    syncDirectoryTreeToActiveLocation,
    toggleDirectoryTree,
  };
}
