import type { EntrySummary, Location } from '../../models';
import { parentLocation } from '../navigation/navigation';

/** Cached expansion/children state for the directory-tree sidebar (task 0139), keyed by
 * location URI. Kept as a plain immutable value so `app-shell.ts` can hold it as ordinary
 * component state, the same way `terminal-state.ts`'s open-drawer set is held. */
export interface TreeChildrenState {
  readonly expanded: ReadonlySet<string>;
  readonly childrenByUri: Readonly<Record<string, readonly EntrySummary[]>>;
  readonly loadingUris: ReadonlySet<string>;
  readonly errorByUri: Readonly<Record<string, string>>;
}

export function createTreeChildrenState(): TreeChildrenState {
  return {
    expanded: new Set(),
    childrenByUri: {},
    loadingUris: new Set(),
    errorByUri: {},
  };
}

/** Marks `uri` expanded/collapsed without touching any cached children. */
export function withExpanded(
  state: TreeChildrenState,
  uri: string,
  expanded: boolean,
): TreeChildrenState {
  const next = new Set(state.expanded);
  if (expanded) {
    next.add(uri);
  } else {
    next.delete(uri);
  }
  return { ...state, expanded: next };
}

/** Caches a node's freshly-fetched children and marks it expanded, loaded, and error-free. */
export function withChildren(
  state: TreeChildrenState,
  uri: string,
  children: readonly EntrySummary[],
): TreeChildrenState {
  const loadingUris = new Set(state.loadingUris);
  loadingUris.delete(uri);
  const { [uri]: _removedError, ...errorByUri } = state.errorByUri;
  return {
    ...withExpanded(state, uri, true),
    childrenByUri: { ...state.childrenByUri, [uri]: children },
    loadingUris,
    errorByUri,
  };
}

export function withLoading(
  state: TreeChildrenState,
  uri: string,
  loading: boolean,
): TreeChildrenState {
  const next = new Set(state.loadingUris);
  if (loading) {
    next.add(uri);
  } else {
    next.delete(uri);
  }
  return { ...state, loadingUris: next };
}

/** Records a failed fetch and collapses the node back, so it doesn't render as expanded with
 * stale/absent children. */
export function withError(
  state: TreeChildrenState,
  uri: string,
  message: string,
): TreeChildrenState {
  const loadingUris = new Set(state.loadingUris);
  loadingUris.delete(uri);
  return {
    ...withExpanded(state, uri, false),
    loadingUris,
    errorByUri: { ...state.errorByUri, [uri]: message },
  };
}

/** One row of the tree's flattened, currently-visible node list — the shape rendering and
 * windowing operate over, since a nested tree's visible rows are not a fixed structure. */
export interface FlatTreeNode {
  readonly location: Location;
  readonly name: string;
  readonly depth: number;
  readonly expanded: boolean;
  readonly loading: boolean;
  readonly error: string | undefined;
  /** `undefined` until the node's children have been fetched at least once. */
  readonly hasChildren: boolean | undefined;
}

interface TreeNodeInput {
  readonly location: Location;
  readonly name: string;
}

/** Flattens the tree rooted at `root` into the list of currently-visible rows: the root, plus
 * (recursively) the cached children of every expanded ancestor. Collapsed or not-yet-loaded
 * nodes contribute no further rows, matching the lazy-expansion requirement — nothing beyond
 * the root is fetched or rendered until a user expands it. */
export function flattenVisibleTree(root: TreeNodeInput, state: TreeChildrenState): FlatTreeNode[] {
  const rows: FlatTreeNode[] = [];
  const visit = (node: TreeNodeInput, depth: number): void => {
    const uri = node.location.uri;
    const children = state.childrenByUri[uri];
    rows.push({
      location: node.location,
      name: node.name,
      depth,
      expanded: state.expanded.has(uri),
      loading: state.loadingUris.has(uri),
      error: state.errorByUri[uri],
      hasChildren: children === undefined ? undefined : children.length > 0,
    });
    if (state.expanded.has(uri) && children !== undefined) {
      for (const child of children) {
        visit({ location: child.location, name: child.name }, depth + 1);
      }
    }
  };
  visit(root, 0);
  return rows;
}

/** Returns `target`'s ancestor chain from its immediate parent up to (and including) `root`,
 * ordered root-first — the sequence of nodes the tree sidebar must expand, in order, so
 * `target` becomes visible. Empty when `target` already equals `root`. Stops at the first
 * fixed point of {@link parentLocation} (a provider root) even if it is reached before `root`,
 * so a target outside `root`'s subtree still yields a terminating (if incomplete) chain rather
 * than never returning. */
export function ancestorChain(root: Location, target: Location): readonly Location[] {
  if (target.uri === root.uri) return [];
  const chain: Location[] = [];
  let current = target;
  for (;;) {
    const parent = parentLocation(current);
    if (parent.uri === current.uri) break;
    if (parent.uri === root.uri) break;
    chain.push(parent);
    current = parent;
  }
  return chain.reverse();
}
