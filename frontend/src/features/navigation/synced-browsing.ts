import type { Location, PaneId, WorkspaceProjection } from '../../models';
import { lastPathSegment, type NavigationController, parentLocation } from './navigation';

/** Where a location sits: a provider-specific base plus its decoded path segments. */
export interface LocationPath {
  readonly providerId: string;
  /** Scheme and authority (or the archive file) that the segments hang off. */
  readonly base: string;
  readonly segments: readonly string[];
}

const decode = (segment: string): string => {
  try {
    return decodeURIComponent(segment);
  } catch {
    return segment;
  }
};

const foldCase = (segment: string): string => segment.toLowerCase();

const splitPath = (path: string): string[] => path.split('/').filter((s) => s.length > 0);

/**
 * Splits a hierarchical location into base + segments. Returns undefined for non-hierarchical
 * locations (search results, the basket) that cannot take part in mirrored navigation.
 */
export function locationPath(location: Location): LocationPath | undefined {
  try {
    if (location.uri.startsWith('search://') || location.providerId === 'basket') return undefined;
    if (location.providerId === 'archive' && location.uri.startsWith('archive://')) {
      const remainder = location.uri.slice('archive://'.length);
      const separator = remainder.indexOf('!');
      if (separator < 0) return undefined;
      return {
        providerId: location.providerId,
        base: `archive://${remainder.slice(0, separator)}!`,
        segments: splitPath(remainder.slice(separator + 1)).map(decode),
      };
    }
    const url = new URL(location.uri);
    return {
      providerId: location.providerId,
      base: `${url.protocol}//${url.host}`,
      segments: splitPath(url.pathname).map(decode),
    };
  } catch {
    return undefined;
  }
}

/** Builds the location `segments` below `base`, the inverse of {@link locationPath}. */
export function joinLocation(
  base: Pick<LocationPath, 'providerId' | 'base'>,
  segments: readonly string[],
): Location {
  if (base.providerId === 'archive' && base.base.startsWith('archive://')) {
    return {
      providerId: base.providerId,
      uri: `${base.base}/${segments.map(encodeURIComponent).join('/')}`,
    };
  }
  const url = new URL(base.base);
  url.pathname = `/${segments.map(encodeURIComponent).join('/')}`;
  return { providerId: base.providerId, uri: url.toString() };
}

const sameBase = (a: LocationPath, b: LocationPath): boolean =>
  a.providerId === b.providerId && a.base === b.base;

/** Number of leading segments equal ignoring case (case-insensitive volumes). */
function commonPrefixLength(a: readonly string[], b: readonly string[]): number {
  let length = 0;
  while (
    length < a.length &&
    length < b.length &&
    foldCase(a[length] as string) === foldCase(b[length] as string)
  ) {
    length += 1;
  }
  return length;
}

/**
 * The segments of `location` below `root`, or undefined when `location` is not inside `root`.
 * Segment names compare case-insensitively so a case-insensitive volume pairs up reliably.
 */
export function relativeSegments(root: Location, location: Location): string[] | undefined {
  const rootPath = locationPath(root);
  const path = locationPath(location);
  if (rootPath === undefined || path === undefined || !sameBase(rootPath, path)) return undefined;
  const common = commonPrefixLength(rootPath.segments, path.segments);
  return common === rootPath.segments.length ? path.segments.slice(common) : undefined;
}

/** True when both locations name the same place, ignoring segment case. */
export function sameLocationPath(a: Location, b: Location): boolean {
  const left = locationPath(a);
  const right = locationPath(b);
  if (left === undefined || right === undefined) return a.uri === b.uri;
  return (
    sameBase(left, right) &&
    left.segments.length === right.segments.length &&
    commonPrefixLength(left.segments, right.segments) === left.segments.length
  );
}

/** Walks `levels` parents up from `location`; undefined when a root is reached first. */
export function ancestorLocation(location: Location, levels: number): Location | undefined {
  let current = location;
  for (let step = 0; step < levels; step += 1) {
    const parent = parentLocation(current);
    if (parent.uri === current.uri) return undefined;
    current = parent;
  }
  return current;
}

/** Off, mirroring, or on-but-currently-apart (counterpart missing or panes elsewhere). */
export type SyncBrowsingState = 'off' | 'synced' | 'desynced';

/** The two locations that correspond to each other when synchronization starts or re-anchors. */
interface Anchors {
  readonly [paneId: string]: Location;
}

/** Persistence of the per-workspace toggle. */
export interface SyncBrowsingStore {
  load(workspaceId: string): boolean;
  save(workspaceId: string, enabled: boolean): void;
}

export interface SyncedBrowsingOptions {
  readonly navigation: NavigationController;
  readonly getWorkspace: () => WorkspaceProjection | undefined;
  /** Finds the directory `name` directly inside `parent` (case-insensitively), if it exists. */
  readonly resolveChild: (parent: Location, name: string) => Promise<Location | undefined>;
  readonly store: SyncBrowsingStore;
  /** Called whenever the observable state changes (to redraw the indicator). */
  readonly onChange?: () => void;
}

export interface SyncedBrowsingController {
  /** The wrapped navigation controller; user navigation through it is mirrored. */
  readonly navigation: NavigationController;
  state(): SyncBrowsingState;
  setEnabled(enabled: boolean): void;
  toggle(): void;
}

/**
 * Task 0255: mirrors navigation between the two panes of the workspace. The paired anchors are
 * the locations that corresponded when sync started; a move in one pane is replayed as the same
 * relative path below the other pane's anchor. A missing counterpart leaves the other pane where
 * it is and marks the state desynchronized; it resyncs as soon as the paths realign.
 */
export function createSyncedBrowsing(options: SyncedBrowsingOptions): SyncedBrowsingController {
  const { navigation } = options;
  let enabledWorkspaceId: string | undefined;
  let enabled = false;
  let desynced = false;
  let anchors: Anchors | undefined;
  /** Serializes mirroring so rapid navigation never interleaves two reconciliations. */
  let queue: Promise<void> = Promise.resolve();

  const setDesynced = (next: boolean): void => {
    if (desynced === next) return;
    desynced = next;
    options.onChange?.();
  };

  const locationOf = (paneId: PaneId): Location | undefined => {
    const pane = options.getWorkspace()?.panesById[paneId];
    return pane?.tabsById[pane.activeTabId]?.location;
  };

  const otherPane = (paneId: PaneId): PaneId | undefined =>
    options.getWorkspace()?.paneOrder.find((id) => id !== paneId);

  /** Reads the persisted flag lazily, per workspace, then keeps it in memory. */
  const ensureWorkspace = (): void => {
    const workspace = options.getWorkspace();
    if (workspace === undefined || workspace.id === enabledWorkspaceId) return;
    enabledWorkspaceId = workspace.id;
    enabled = options.store.load(workspace.id);
    desynced = false;
    anchors = enabled ? captureAnchors() : undefined;
  };

  function captureAnchors(): Anchors | undefined {
    const workspace = options.getWorkspace();
    if (workspace === undefined) return undefined;
    const entries: [string, Location][] = [];
    for (const paneId of workspace.paneOrder.slice(0, 2)) {
      const location = locationOf(paneId);
      if (location !== undefined) entries.push([paneId, location]);
    }
    return entries.length === 2 ? Object.fromEntries(entries) : undefined;
  }

  /** Resolves `segments` below `start` one level at a time; undefined if any is missing. */
  async function descend(
    start: Location,
    segments: readonly string[],
  ): Promise<Location | undefined> {
    let current = start;
    for (const segment of segments) {
      const child = await options.resolveChild(current, segment);
      if (child === undefined) return undefined;
      current = child;
    }
    return current;
  }

  async function reconcile(paneId: PaneId): Promise<void> {
    const otherId = otherPane(paneId);
    const here = locationOf(paneId);
    const there = otherId === undefined ? undefined : locationOf(otherId);
    if (
      anchors === undefined ||
      otherId === undefined ||
      here === undefined ||
      there === undefined
    ) {
      return;
    }
    const hereAnchor = anchors[paneId];
    const thereAnchor = anchors[otherId];
    if (hereAnchor === undefined || thereAnchor === undefined) return;
    const relative = relativeSegments(hereAnchor, here);
    if (relative !== undefined) {
      const thereRelative = relativeSegments(thereAnchor, there);
      if (thereRelative !== undefined) {
        const thereBase = locationPath(thereAnchor);
        if (thereBase === undefined) return;
        const common = commonPrefixLength(relative, thereRelative);
        if (common === relative.length && common === thereRelative.length) {
          setDesynced(false);
          return;
        }
        // Keep the part the other pane already shares (using its real spelling), resolve the rest.
        const start = joinLocation(thereBase, [
          ...thereBase.segments,
          ...thereRelative.slice(0, common),
        ]);
        const target = await descend(start, relative.slice(common));
        if (target === undefined) {
          setDesynced(true);
          return;
        }
        await moveTo(otherId, target, there);
        setDesynced(false);
        return;
      }
      // The other pane left its anchor tree: head back to the counterpart of this location.
      const target = await descend(thereAnchor, relative);
      if (target === undefined) {
        setDesynced(true);
        return;
      }
      await moveTo(otherId, target, there);
      setDesynced(false);
      return;
    }
    // This pane went above its anchor: take the other pane the same number of levels up.
    const levels = relativeSegments(here, hereAnchor)?.length;
    const raised = levels === undefined ? undefined : ancestorLocation(thereAnchor, levels);
    if (raised === undefined) {
      setDesynced(true);
      return;
    }
    anchors = { [paneId]: here, [otherId]: raised };
    await moveTo(otherId, raised, there);
    setDesynced(false);
  }

  async function moveTo(paneId: PaneId, target: Location, from: Location): Promise<void> {
    if (sameLocationPath(target, from)) return;
    const ascending = relativeSegments(target, from) !== undefined;
    await navigation.navigate(paneId, target, ascending ? lastPathSegment(from) : undefined, {
      preserveCurrentOnError: true,
    });
  }

  /** Runs one user navigation, then replays it in the other pane. */
  async function mirrored(paneId: PaneId, run: () => Promise<void>): Promise<void> {
    ensureWorkspace();
    const before = locationOf(paneId);
    await run();
    if (!enabled) return;
    const after = locationOf(paneId);
    if (before !== undefined && after !== undefined && sameLocationPath(before, after)) return;
    queue = queue.then(() => reconcile(paneId)).catch(() => setDesynced(true));
    await queue;
  }

  const wrapped: NavigationController = {
    ...navigation,
    navigate: (paneId, location, preferredCursorName, navigationOptions) =>
      mirrored(paneId, () =>
        navigation.navigate(paneId, location, preferredCursorName, navigationOptions),
      ),
    parent: (paneId) => mirrored(paneId, () => navigation.parent(paneId)),
    back: (paneId) => mirrored(paneId, () => navigation.back(paneId)),
    forward: (paneId) => mirrored(paneId, () => navigation.forward(paneId)),
  };

  const setEnabled = (next: boolean): void => {
    ensureWorkspace();
    const workspace = options.getWorkspace();
    if (workspace === undefined) return;
    enabled = next;
    desynced = false;
    anchors = next ? captureAnchors() : undefined;
    options.store.save(workspace.id, next);
    options.onChange?.();
  };

  return {
    navigation: wrapped,
    state: () => {
      ensureWorkspace();
      if (!enabled) return 'off';
      return desynced ? 'desynced' : 'synced';
    },
    setEnabled,
    toggle: () => {
      ensureWorkspace();
      setEnabled(!enabled);
    },
  };
}

/** Browser-storage backed store; storage failures just mean the toggle lasts for the session. */
export function createLocalStorageSyncStore(
  storage: Pick<Storage, 'getItem' | 'setItem'>,
): SyncBrowsingStore {
  const key = (workspaceId: string): string => `fm.syncedBrowsing.${workspaceId}`;
  return {
    load: (workspaceId) => {
      try {
        return storage.getItem(key(workspaceId)) === '1';
      } catch {
        return false;
      }
    },
    save: (workspaceId, enabled) => {
      try {
        storage.setItem(key(workspaceId), enabled ? '1' : '0');
      } catch {
        // Unavailable storage: not persisted.
      }
    },
  };
}
