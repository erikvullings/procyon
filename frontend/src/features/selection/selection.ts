import type { EntryId, EntrySummary, Location } from '../../models';
import { isParentEntry } from '../panes/parent-entry';

/** Stable-ID selection state for one pane. */
export interface SelectionState {
  readonly selectedEntryIds: readonly EntryId[];
  readonly cursorEntryId?: EntryId;
  readonly anchorEntryId?: EntryId;
  /** Entries frozen from a prior multi-selection so Shift-extend unions rather than replaces. */
  readonly baseSelectedEntryIds?: readonly EntryId[];
}

/** Framework-independent transitions supported by the directory selection model. */
export type SelectionAction =
  | { readonly type: 'moveCursor'; readonly offset: number }
  | { readonly type: 'moveCursorTo'; readonly edge: 'first' | 'last' }
  | { readonly type: 'setCursor'; readonly entryId: EntryId }
  /** Moves the cursor to `entryId` without marking it (Total Commander parity: a plain click only
   * repositions the cursor). Existing visible marks remain independent of the cursor. */
  | { readonly type: 'positionCursor'; readonly entryId: EntryId }
  | { readonly type: 'selectOnly'; readonly entryId: EntryId }
  /** Signals that a typed prefix should also be searched against entries not loaded yet (task:
   * type-to-select only searching loaded entries) - a pure no-op for the reducer itself; the
   * workspace layer intercepts it to background-load the rest of the directory and select the
   * true first match once it's in, exactly like `moveCursorTo`'s `'last'` edge does. */
  | { readonly type: 'typeaheadPending'; readonly prefix: string }
  | { readonly type: 'toggle'; readonly entryId: EntryId }
  /** Toggles `entryId`'s selection and moves the cursor by `offset` in one atomic transition
   * (Insert/Space, Total Commander parity) - a single reducer step rather than a `toggle` dispatch
   * immediately followed by a `moveCursor` dispatch, which observably dropped the toggle in
   * practice (see the 2026-08 investigation in `pane.ts`'s dispatch comment for this command). */
  | { readonly type: 'toggleAndAdvance'; readonly entryId: EntryId; readonly offset: number }
  | { readonly type: 'extendRange'; readonly offset: number }
  /** Applies Shift+Arrow range semantics within a filtered ordering, so entries excluded by an
   * active typeahead prefix cannot enter the selection merely because they sit between matches. */
  | {
      readonly type: 'extendRangeWithin';
      readonly orderedEntryIds: readonly EntryId[];
      readonly offset: number;
    }
  | { readonly type: 'extendRangeTo'; readonly entryId: EntryId }
  | { readonly type: 'selectAll' }
  | { readonly type: 'invert' }
  | { readonly type: 'selectByMask'; readonly matchingEntryIds: readonly EntryId[] }
  | { readonly type: 'deselectByMask'; readonly matchingEntryIds: readonly EntryId[] }
  | { readonly type: 'clear' }
  | { readonly type: 'prune'; readonly removedEntryIds: readonly EntryId[] }
  /** Replaces the selection wholesale, filtered to currently visible entries (Numpad `/`). */
  | { readonly type: 'restore'; readonly entryIds: readonly EntryId[] };

export const emptySelection: SelectionState = { selectedEntryIds: [] };

function clampedIndex(index: number, entryIds: readonly EntryId[]): number | undefined {
  if (entryIds.length === 0) {
    return undefined;
  }
  return Math.max(0, Math.min(index, entryIds.length - 1));
}

function cursorIndex(state: SelectionState, entryIds: readonly EntryId[]): number {
  const index = state.cursorEntryId === undefined ? -1 : entryIds.indexOf(state.cursorEntryId);
  return index < 0 ? 0 : index;
}

/** Applies one selection transition using the entries in their current visible order. */
export function reduceSelection(
  state: SelectionState,
  action: SelectionAction,
  orderedEntryIds: readonly EntryId[],
): SelectionState {
  const visibleIds = new Set(orderedEntryIds);
  switch (action.type) {
    case 'moveCursor': {
      const index = clampedIndex(
        cursorIndex(state, orderedEntryIds) + action.offset,
        orderedEntryIds,
      );
      const entryId = index === undefined ? undefined : orderedEntryIds[index];
      if (entryId === undefined) return state;
      // Keep an existing selection when navigating without Shift so users can skip entries
      // without losing marks made with Space, Ctrl/Cmd-click, or range selection.
      if (
        state.selectedEntryIds.length > 0 &&
        state.selectedEntryIds.every((selectedId) => visibleIds.has(selectedId))
      ) {
        return {
          ...state,
          cursorEntryId: entryId,
          anchorEntryId: entryId,
          baseSelectedEntryIds: state.selectedEntryIds,
        };
      }
      const { baseSelectedEntryIds: _b1, ...withoutBase1 } = state;
      return {
        ...withoutBase1,
        selectedEntryIds: [],
        cursorEntryId: entryId,
        anchorEntryId: entryId,
      };
    }
    case 'moveCursorTo': {
      const entryId = action.edge === 'first' ? orderedEntryIds[0] : orderedEntryIds.at(-1);
      if (entryId === undefined) return state;
      if (
        state.selectedEntryIds.length > 0 &&
        state.selectedEntryIds.every((selectedId) => visibleIds.has(selectedId))
      ) {
        return {
          ...state,
          cursorEntryId: entryId,
          anchorEntryId: entryId,
          baseSelectedEntryIds: state.selectedEntryIds,
        };
      }
      const { baseSelectedEntryIds: _b2, ...withoutBase2 } = state;
      return {
        ...withoutBase2,
        selectedEntryIds: [],
        cursorEntryId: entryId,
        anchorEntryId: entryId,
      };
    }
    case 'setCursor':
      return { ...state, cursorEntryId: action.entryId };
    case 'positionCursor': {
      if (
        state.selectedEntryIds.length > 0 &&
        state.selectedEntryIds.every((selectedId) => visibleIds.has(selectedId))
      ) {
        return {
          ...state,
          cursorEntryId: action.entryId,
          anchorEntryId: action.entryId,
          baseSelectedEntryIds: state.selectedEntryIds,
        };
      }
      const { baseSelectedEntryIds: _b3, ...withoutBase3 } = state;
      return {
        ...withoutBase3,
        selectedEntryIds: [],
        cursorEntryId: action.entryId,
        anchorEntryId: action.entryId,
      };
    }
    case 'selectOnly':
      return {
        selectedEntryIds: [action.entryId],
        cursorEntryId: action.entryId,
        anchorEntryId: action.entryId,
      };
    case 'typeaheadPending':
      return state;
    case 'toggle': {
      const selected = new Set(state.selectedEntryIds);
      if (selected.has(action.entryId)) {
        selected.delete(action.entryId);
      } else {
        selected.add(action.entryId);
      }
      return {
        selectedEntryIds: [...selected],
        cursorEntryId: action.entryId,
        anchorEntryId: action.entryId,
      };
    }
    case 'toggleAndAdvance': {
      const selected = new Set(state.selectedEntryIds);
      if (selected.has(action.entryId)) {
        selected.delete(action.entryId);
      } else {
        selected.add(action.entryId);
      }
      const toggledEntryIndex = orderedEntryIds.indexOf(action.entryId);
      const nextIndex = clampedIndex(
        (toggledEntryIndex < 0 ? 0 : toggledEntryIndex) + action.offset,
        orderedEntryIds,
      );
      const nextCursorEntryId =
        (nextIndex === undefined ? undefined : orderedEntryIds[nextIndex]) ?? action.entryId;
      return {
        selectedEntryIds: [...selected],
        cursorEntryId: nextCursorEntryId,
        anchorEntryId: nextCursorEntryId,
      };
    }
    case 'extendRange': {
      if (orderedEntryIds.length === 0) {
        return state;
      }
      const currentIndex = cursorIndex(state, orderedEntryIds);
      const requestedIndex = currentIndex + action.offset;
      const nextIndex = clampedIndex(requestedIndex, orderedEntryIds) ?? currentIndex;
      // The cursor can't move past the first/last row, so there's no future press left to "catch
      // up" and select it via the departed-row mechanism below - select it immediately instead of
      // leaving it a permanently-unreachable bare cursor (e.g. Shift+Down at the last row).
      const clampedAtBoundary = requestedIndex !== nextIndex;
      const anchorEntryId =
        state.anchorEntryId ?? state.cursorEntryId ?? orderedEntryIds[currentIndex];
      const anchorIndex =
        anchorEntryId === undefined ? currentIndex : orderedEntryIds.indexOf(anchorEntryId);
      const resolvedAnchorIndex = anchorIndex < 0 ? currentIndex : anchorIndex;
      // Shift+Arrow selects the row being departed and leaves the destination as a bare cursor.
      const rangeIds =
        nextIndex > resolvedAnchorIndex
          ? orderedEntryIds.slice(
              resolvedAnchorIndex,
              clampedAtBoundary ? nextIndex + 1 : nextIndex,
            )
          : nextIndex < resolvedAnchorIndex
            ? orderedEntryIds.slice(
                clampedAtBoundary ? nextIndex : nextIndex + 1,
                resolvedAnchorIndex + 1,
              )
            : orderedEntryIds.slice(resolvedAnchorIndex, resolvedAnchorIndex + 1);
      const base = state.baseSelectedEntryIds;
      const baseSet = base !== undefined ? new Set(base) : undefined;
      const merged =
        baseSet !== undefined
          ? orderedEntryIds.filter((id) => baseSet.has(id) || rangeIds.includes(id))
          : rangeIds;
      return {
        selectedEntryIds: merged,
        ...(orderedEntryIds[nextIndex] === undefined
          ? {}
          : { cursorEntryId: orderedEntryIds[nextIndex] }),
        ...(anchorEntryId === undefined ? {} : { anchorEntryId }),
        ...(base !== undefined ? { baseSelectedEntryIds: base } : {}),
      };
    }
    case 'extendRangeWithin':
      return reduceSelection(
        state,
        { type: 'extendRange', offset: action.offset },
        action.orderedEntryIds,
      );
    case 'extendRangeTo': {
      const targetIndex = orderedEntryIds.indexOf(action.entryId);
      if (targetIndex < 0) {
        return state;
      }
      const anchorEntryId = state.anchorEntryId ?? state.cursorEntryId ?? action.entryId;
      const anchorIndex = orderedEntryIds.indexOf(anchorEntryId);
      const rangeStart = Math.min(anchorIndex < 0 ? targetIndex : anchorIndex, targetIndex);
      const rangeEnd = Math.max(anchorIndex < 0 ? targetIndex : anchorIndex, targetIndex);
      const rangeIds = orderedEntryIds.slice(rangeStart, rangeEnd + 1);
      const base = state.baseSelectedEntryIds;
      const baseSet = base !== undefined ? new Set(base) : undefined;
      const merged =
        baseSet !== undefined
          ? orderedEntryIds.filter((id) => baseSet.has(id) || rangeIds.includes(id))
          : rangeIds;
      return {
        selectedEntryIds: merged,
        cursorEntryId: action.entryId,
        anchorEntryId,
        ...(base !== undefined ? { baseSelectedEntryIds: base } : {}),
      };
    }
    case 'selectAll':
      return { ...state, selectedEntryIds: [...orderedEntryIds] };
    case 'invert': {
      const selected = new Set(state.selectedEntryIds);
      return {
        ...state,
        selectedEntryIds: orderedEntryIds.filter((entryId) => !selected.has(entryId)),
      };
    }
    case 'selectByMask': {
      if (action.matchingEntryIds.length === 0) return state;
      const selected = new Set([...state.selectedEntryIds, ...action.matchingEntryIds]);
      const visible = orderedEntryIds.filter((entryId) => selected.has(entryId));
      const visibleIds = new Set(orderedEntryIds);
      const hidden = state.selectedEntryIds.filter((entryId) => !visibleIds.has(entryId));
      return { ...state, selectedEntryIds: [...visible, ...hidden] };
    }
    case 'deselectByMask': {
      if (action.matchingEntryIds.length === 0) return state;
      const deselected = new Set(action.matchingEntryIds);
      const remaining = state.selectedEntryIds.filter((entryId) => !deselected.has(entryId));
      const remainingIds = new Set(remaining);
      const visible = orderedEntryIds.filter((entryId) => remainingIds.has(entryId));
      const visibleIds = new Set(orderedEntryIds);
      const hidden = remaining.filter((entryId) => !visibleIds.has(entryId));
      return { ...state, selectedEntryIds: [...visible, ...hidden] };
    }
    case 'clear': {
      const { anchorEntryId: _anchorEntryId, baseSelectedEntryIds: _base, ...withoutMeta } = state;
      return { ...withoutMeta, selectedEntryIds: [] };
    }
    case 'restore': {
      const visible = new Set(orderedEntryIds);
      const restored = action.entryIds.filter((entryId) => visible.has(entryId));
      return { ...state, selectedEntryIds: restored };
    }
    case 'prune': {
      const removed = new Set(action.removedEntryIds);
      const removedCursorEntryId =
        state.cursorEntryId !== undefined && removed.has(state.cursorEntryId)
          ? state.cursorEntryId
          : undefined;
      let fallbackCursorEntryId: EntryId | undefined;
      if (removedCursorEntryId !== undefined) {
        const cursorIndex = orderedEntryIds.indexOf(removedCursorEntryId);
        if (cursorIndex >= 0) {
          fallbackCursorEntryId = orderedEntryIds
            .slice(0, cursorIndex)
            .findLast((entryId) => !removed.has(entryId));
          fallbackCursorEntryId ??= orderedEntryIds
            .slice(cursorIndex + 1)
            .find((entryId) => !removed.has(entryId));
        }
      }
      return {
        selectedEntryIds: state.selectedEntryIds.filter((entryId) => !removed.has(entryId)),
        ...(removedCursorEntryId !== undefined
          ? fallbackCursorEntryId === undefined
            ? {}
            : { cursorEntryId: fallbackCursorEntryId }
          : state.cursorEntryId === undefined
            ? {}
            : { cursorEntryId: state.cursorEntryId }),
        ...(state.anchorEntryId === undefined || removed.has(state.anchorEntryId)
          ? fallbackCursorEntryId === undefined
            ? {}
            : { anchorEntryId: fallbackCursorEntryId }
          : { anchorEntryId: state.anchorEntryId }),
      };
    }
  }
}

/**
 * Retrieves the directory entries that are currently selected.
 * Returns entries in the same order as the directory listing, not the selection order.
 */ export function getSelectedEntries(
  selection: SelectionState | undefined,
  entries: readonly EntrySummary[],
): readonly EntrySummary[] {
  if (selection === undefined || selection.selectedEntryIds.length === 0) {
    return [];
  }
  const idSet = new Set(selection.selectedEntryIds);
  return entries.filter((entry) => idSet.has(entry.id) === true && !isParentEntry(entry.id));
}

/**
 * `getSelectedEntries`, falling back to the cursor entry when nothing is explicitly marked -
 * Total Commander convention: a plain click only moves the cursor (see `setCursor` above), so a
 * command invoked right after (Delete, Copy, cut/paste, pack, ...) must still act on the file the
 * cursor is sitting on rather than silently doing nothing because no row was ever marked.
 */
export function getSelectedEntriesOrCursor(
  selection: SelectionState | undefined,
  entries: readonly EntrySummary[],
): readonly EntrySummary[] {
  const selected = getSelectedEntries(selection, entries);
  if (selected.length > 0) return selected;
  const cursor =
    selection?.cursorEntryId === undefined
      ? undefined
      : entries.find((entry) => entry.id === selection.cursorEntryId);
  return cursor === undefined ? [] : [cursor];
}

/**
 * Retrieves the locations of the currently selected entries.
 * Equivalent to `getSelectedEntries(selection, entries).map(e => e.location)`.
 */ export function getSelectedEntryLocations(
  selection: SelectionState | undefined,
  entries: readonly EntrySummary[],
): readonly Location[] {
  return getSelectedEntries(selection, entries).map((entry) => entry.location);
}
