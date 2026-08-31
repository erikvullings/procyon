import type {
  ComparisonCriteria,
  ComparisonEntry,
  EntryId,
  EntrySummary,
  Location,
  PaneId,
} from '../../models';

/** Live state for the directory comparison overlay (spec §16 milestone 5, task 0075). */
export interface ComparisonState {
  readonly comparisonId?: string;
  readonly criteria?: ComparisonCriteria;
  readonly leftRoot?: Location;
  readonly rightRoot?: Location;
  readonly leftPaneId?: PaneId;
  readonly rightPaneId?: PaneId;
  readonly statusByRelativePath: ReadonlyMap<string, ComparisonEntry>;
  readonly isComplete: boolean;
  readonly warningsCount: number;
  readonly differencesOnly: boolean;
  readonly error?: string;
}

/** No comparison running; every pane renders exactly as it does today. */
export function initialComparisonState(): ComparisonState {
  return {
    statusByRelativePath: new Map(),
    isComplete: false,
    warningsCount: 0,
    differencesOnly: false,
  };
}

/** Replaces any previous comparison with a freshly started one. */
export function withComparisonStarted(params: {
  comparisonId: string;
  criteria: ComparisonCriteria;
  leftRoot: Location;
  rightRoot: Location;
  leftPaneId: PaneId;
  rightPaneId: PaneId;
}): ComparisonState {
  return {
    comparisonId: params.comparisonId,
    criteria: params.criteria,
    leftRoot: params.leftRoot,
    rightRoot: params.rightRoot,
    leftPaneId: params.leftPaneId,
    rightPaneId: params.rightPaneId,
    statusByRelativePath: new Map(),
    isComplete: false,
    warningsCount: 0,
    differencesOnly: false,
  };
}

/** Merges a streamed results batch into the running comparison. A no-op if `comparisonId`
 * does not match the currently tracked comparison (a stale/cancelled batch arriving late). */
export function withComparisonBatch(
  state: ComparisonState,
  comparisonId: string,
  entries: readonly ComparisonEntry[],
  isComplete: boolean,
  warningsCount: number,
): ComparisonState {
  if (state.comparisonId !== comparisonId) return state;
  const next = new Map(state.statusByRelativePath);
  for (const entry of entries) {
    next.set(entry.relativePath, entry);
  }
  return { ...state, statusByRelativePath: next, isComplete, warningsCount };
}

/** Clears the active comparison; every pane returns to its plain, unannotated view. */
export function withComparisonCleared(): ComparisonState {
  return initialComparisonState();
}

export function withDifferencesOnly(state: ComparisonState, value: boolean): ComparisonState {
  return { ...state, differencesOnly: value };
}

export function withComparisonError(state: ComparisonState, message: string): ComparisonState {
  return { ...state, error: message };
}

/** Which side (if any) `paneId` represents in the active comparison. */
export function sideForPane(state: ComparisonState, paneId: PaneId): 'left' | 'right' | undefined {
  if (state.leftPaneId === paneId) return 'left';
  if (state.rightPaneId === paneId) return 'right';
  return undefined;
}

/** An entry's path relative to `root`, or `undefined` when it does not fall under it. Mirrors
 * `fm_comparison::path`'s `/`-joined relative-path shape, so results match server-side entries. */
export function relativePathUnder(entryUri: string, root: Location): string | undefined {
  const rootUri = root.uri.endsWith('/') ? root.uri.slice(0, -1) : root.uri;
  if (entryUri === rootUri) return '';
  if (!entryUri.startsWith(`${rootUri}/`)) return undefined;
  return decodeURIComponent(entryUri.slice(rootUri.length + 1));
}

/** Looks up an entry's comparison outcome for the pane it is displayed in. `undefined` when the
 * pane isn't part of the active comparison, the entry falls outside its root, or nothing is known
 * about it yet. */
export function statusForEntry(
  state: ComparisonState,
  paneId: PaneId,
  entryUri: string,
): ComparisonEntry | undefined {
  const side = sideForPane(state, paneId);
  if (side === undefined) return undefined;
  const root = side === 'left' ? state.leftRoot : state.rightRoot;
  if (root === undefined) return undefined;
  const relativePath = relativePathUnder(entryUri, root);
  if (relativePath === undefined) return undefined;
  return state.statusByRelativePath.get(relativePath);
}

/** Entry ids among `entries` whose comparison outcome is known and not `identical`, i.e. the
 * rows a completed comparison should mark selected in `paneId` (Total-Commander-style "Compare
 * directories": differing entries are selected directly in both panes rather than reported
 * through a separate dialog or per-row badge). */
export function differingEntryIds(
  state: ComparisonState,
  paneId: PaneId,
  entries: readonly EntrySummary[],
): readonly EntryId[] {
  return entries
    .filter((entry) => {
      const outcome = statusForEntry(state, paneId, entry.location.uri);
      return outcome !== undefined && outcome.status !== 'identical';
    })
    .map((entry) => entry.id);
}
