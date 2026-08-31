import { describe, expect, it } from 'vitest';

import type { EntryId, EntrySummary, Location } from '../../models';
import {
  emptySelection,
  getSelectedEntries,
  getSelectedEntryLocations,
  reduceSelection,
  type SelectionState,
} from './selection';

const ids = (...values: string[]): readonly EntryId[] => values;

describe('selection reducer', () => {
  it('moves the cursor without dropping a lone marked row', () => {
    expect(
      reduceSelection(
        { ...emptySelection, selectedEntryIds: ['b'], cursorEntryId: 'b' },
        { type: 'moveCursor', offset: 1 },
        ids('a', 'b', 'c'),
      ),
    ).toEqual({
      selectedEntryIds: ['b'],
      cursorEntryId: 'c',
      anchorEntryId: 'c',
      baseSelectedEntryIds: ['b'],
    });
  });

  it('moves by a page and clamps at the list boundary', () => {
    expect(
      reduceSelection(
        { ...emptySelection, cursorEntryId: 'b' },
        { type: 'moveCursor', offset: 20 },
        ids('a', 'b', 'c'),
      ).cursorEntryId,
    ).toBe('c');
  });

  it('moves to the first or last entry', () => {
    const first = reduceSelection(
      { ...emptySelection, cursorEntryId: 'b' },
      { type: 'moveCursorTo', edge: 'first' },
      ids('a', 'b', 'c'),
    );
    expect(first.cursorEntryId).toBe('a');
    expect(
      reduceSelection(first, { type: 'moveCursorTo', edge: 'last' }, ids('a', 'b', 'c'))
        .cursorEntryId,
    ).toBe('c');
  });

  it('selects one entry and establishes a range anchor', () => {
    expect(
      reduceSelection(emptySelection, { type: 'selectOnly', entryId: 'b' }, ids('a', 'b', 'c')),
    ).toEqual({
      selectedEntryIds: ['b'],
      cursorEntryId: 'b',
      anchorEntryId: 'b',
    });
  });

  it('toggles entries into and out of a discontinuous selection', () => {
    const selected = reduceSelection(
      emptySelection,
      { type: 'toggle', entryId: 'a' },
      ids('a', 'b', 'c'),
    );
    const discontinuous = reduceSelection(
      selected,
      { type: 'toggle', entryId: 'c' },
      ids('a', 'b', 'c'),
    );
    expect(discontinuous.selectedEntryIds).toEqual(['a', 'c']);
    expect(
      reduceSelection(discontinuous, { type: 'toggle', entryId: 'a' }, ids('a', 'b', 'c'))
        .selectedEntryIds,
    ).toEqual(['c']);
  });

  it('toggles the given entry and advances the cursor in one atomic step (Insert/Space)', () => {
    const toggled = reduceSelection(
      { ...emptySelection, cursorEntryId: 'a' },
      { type: 'toggleAndAdvance', entryId: 'a', offset: 1 },
      ids('a', 'b', 'c'),
    );
    expect(toggled).toEqual({ selectedEntryIds: ['a'], cursorEntryId: 'b', anchorEntryId: 'b' });
  });

  it('toggleAndAdvance can toggle an entry back off and still advance', () => {
    const initial: SelectionState = {
      selectedEntryIds: ['a'],
      cursorEntryId: 'a',
      anchorEntryId: 'a',
    };
    const toggled = reduceSelection(
      initial,
      { type: 'toggleAndAdvance', entryId: 'a', offset: 1 },
      ids('a', 'b', 'c'),
    );
    expect(toggled).toEqual({ selectedEntryIds: [], cursorEntryId: 'b', anchorEntryId: 'b' });
  });

  it('toggleAndAdvance clamps the cursor at the last entry instead of losing the toggle', () => {
    const toggled = reduceSelection(
      { ...emptySelection, cursorEntryId: 'c' },
      { type: 'toggleAndAdvance', entryId: 'c', offset: 1 },
      ids('a', 'b', 'c'),
    );
    expect(toggled).toEqual({ selectedEntryIds: ['c'], cursorEntryId: 'c', anchorEntryId: 'c' });
  });

  it('extends a range from its stable anchor across a sort change', () => {
    const initial: SelectionState = {
      selectedEntryIds: ['b'],
      cursorEntryId: 'b',
      anchorEntryId: 'b',
    };
    const extended = reduceSelection(
      initial,
      { type: 'extendRange', offset: 1 },
      ids('a', 'b', 'c', 'd'),
    );
    expect(extended.selectedEntryIds).toEqual(['b']);

    expect(
      reduceSelection(extended, { type: 'extendRange', offset: -1 }, ids('d', 'b', 'a', 'c')),
    ).toEqual({
      selectedEntryIds: ['b'],
      cursorEntryId: 'a',
      anchorEntryId: 'b',
    });
  });

  it('selects rows departed by Shift+Down but not the cursor destination', () => {
    let state: SelectionState = {
      selectedEntryIds: ['0'],
      cursorEntryId: '0',
      anchorEntryId: '0',
    };
    const ordered = ids('0', '1', '2', '3', '4', '5', '6', '7', '8', '9');
    for (let press = 0; press < 3; press += 1) {
      state = reduceSelection(state, { type: 'extendRange', offset: 1 }, ordered);
    }
    expect(state.selectedEntryIds).toEqual(['0', '1', '2']);
    expect(state.cursorEntryId).toBe('3');

    state = reduceSelection(state, { type: 'moveCursor', offset: 1 }, ordered);
    expect(state.selectedEntryIds).toEqual(['0', '1', '2']);
    expect(state.cursorEntryId).toBe('4');
  });

  it('Shift+Down selects the last row once the cursor can no longer move past it', () => {
    const ordered = ids('0', '1', '2');
    let state: SelectionState = { selectedEntryIds: ['0'], cursorEntryId: '0', anchorEntryId: '0' };
    // Departed rows land in the selection one press behind the cursor; at the last row there's no
    // further press to "catch up" and select it, so it must be included immediately instead.
    state = reduceSelection(state, { type: 'extendRange', offset: 1 }, ordered);
    state = reduceSelection(state, { type: 'extendRange', offset: 1 }, ordered);
    expect(state.selectedEntryIds).toEqual(['0', '1']);
    expect(state.cursorEntryId).toBe('2');

    state = reduceSelection(state, { type: 'extendRange', offset: 1 }, ordered);
    expect(state.selectedEntryIds).toEqual(['0', '1', '2']);
    expect(state.cursorEntryId).toBe('2');

    // Repeated presses at the boundary stay idempotent rather than dropping the selection.
    state = reduceSelection(state, { type: 'extendRange', offset: 1 }, ordered);
    expect(state.selectedEntryIds).toEqual(['0', '1', '2']);
    expect(state.cursorEntryId).toBe('2');
  });

  it('Shift+Up selects the first row once the cursor can no longer move past it', () => {
    const ordered = ids('0', '1', '2');
    let state: SelectionState = { selectedEntryIds: ['2'], cursorEntryId: '2', anchorEntryId: '2' };
    state = reduceSelection(state, { type: 'extendRange', offset: -1 }, ordered);
    state = reduceSelection(state, { type: 'extendRange', offset: -1 }, ordered);
    expect(state.selectedEntryIds).toEqual(['1', '2']);
    expect(state.cursorEntryId).toBe('0');

    state = reduceSelection(state, { type: 'extendRange', offset: -1 }, ordered);
    expect(state.selectedEntryIds).toEqual(['0', '1', '2']);
    expect(state.cursorEntryId).toBe('0');

    state = reduceSelection(state, { type: 'extendRange', offset: -1 }, ordered);
    expect(state.selectedEntryIds).toEqual(['0', '1', '2']);
    expect(state.cursorEntryId).toBe('0');
  });

  it('Shift+Down selects a single-row list immediately, with no room to move at all', () => {
    const state = reduceSelection(
      { ...emptySelection, cursorEntryId: 'a', anchorEntryId: 'a' },
      { type: 'extendRange', offset: 1 },
      ids('a'),
    );
    expect(state.selectedEntryIds).toEqual(['a']);
    expect(state.cursorEntryId).toBe('a');
  });

  it('extends a filtered range without selecting entries between matches', () => {
    const ordered = ids('first.dmg', 'notes.txt', 'second.dmg', 'photo.jpg', 'third.dmg');
    const matching = ids('first.dmg', 'second.dmg', 'third.dmg');
    let state: SelectionState = {
      selectedEntryIds: ['first.dmg'],
      cursorEntryId: 'first.dmg',
      anchorEntryId: 'first.dmg',
    };

    state = reduceSelection(
      state,
      { type: 'extendRangeWithin', orderedEntryIds: matching, offset: 1 },
      ordered,
    );
    expect(state.selectedEntryIds).toEqual(['first.dmg']);
    expect(state.cursorEntryId).toBe('second.dmg');

    state = reduceSelection(
      state,
      { type: 'extendRangeWithin', orderedEntryIds: matching, offset: 1 },
      ordered,
    );
    expect(state.selectedEntryIds).toEqual(['first.dmg', 'second.dmg']);
    expect(state.cursorEntryId).toBe('third.dmg');

    state = reduceSelection(
      state,
      { type: 'extendRangeWithin', orderedEntryIds: matching, offset: 1 },
      ordered,
    );
    expect(state.selectedEntryIds).toEqual(['first.dmg', 'second.dmg', 'third.dmg']);
    expect(state.cursorEntryId).toBe('third.dmg');
  });

  it('extends a range to a clicked entry and keeps the anchor when clicking back and forth', () => {
    const initial: SelectionState = {
      selectedEntryIds: ['b'],
      cursorEntryId: 'b',
      anchorEntryId: 'b',
    };
    const extended = reduceSelection(
      initial,
      { type: 'extendRangeTo', entryId: 'd' },
      ids('a', 'b', 'c', 'd'),
    );
    expect(extended).toEqual({
      selectedEntryIds: ['b', 'c', 'd'],
      cursorEntryId: 'd',
      anchorEntryId: 'b',
    });

    expect(
      reduceSelection(extended, { type: 'extendRangeTo', entryId: 'a' }, ids('a', 'b', 'c', 'd')),
    ).toEqual({
      selectedEntryIds: ['a', 'b'],
      cursorEntryId: 'a',
      anchorEntryId: 'b',
    });
  });

  it('selects all and inverts the current visible entries', () => {
    const all = reduceSelection(emptySelection, { type: 'selectAll' }, ids('a', 'b', 'c'));
    expect(all.selectedEntryIds).toEqual(['a', 'b', 'c']);
    expect(
      reduceSelection(
        { ...all, selectedEntryIds: ['a', 'c'] },
        { type: 'invert' },
        ids('a', 'b', 'c'),
      ).selectedEntryIds,
    ).toEqual(['b']);
  });

  it('adds matching visible entries without disturbing hidden or existing selections', () => {
    const result = reduceSelection(
      { selectedEntryIds: ['hidden', 'b'] },
      { type: 'selectByMask', matchingEntryIds: ids('c', 'a') },
      ids('a', 'b', 'c'),
    );
    expect(result.selectedEntryIds).toEqual(['a', 'b', 'c', 'hidden']);
  });

  it('removes matching visible entries without disturbing the rest of the selection', () => {
    const result = reduceSelection(
      { selectedEntryIds: ['hidden', 'a', 'b', 'c'] },
      { type: 'deselectByMask', matchingEntryIds: ids('c', 'a') },
      ids('a', 'b', 'c'),
    );
    expect(result.selectedEntryIds).toEqual(['b', 'hidden']);
  });

  it('handles no mask matches and all visible entries matching', () => {
    const initial: SelectionState = { selectedEntryIds: ['hidden', 'b'] };
    expect(
      reduceSelection(initial, { type: 'selectByMask', matchingEntryIds: [] }, ids('a', 'b')),
    ).toEqual(initial);
    expect(
      reduceSelection(
        initial,
        { type: 'selectByMask', matchingEntryIds: ids('a', 'b') },
        ids('a', 'b'),
      ).selectedEntryIds,
    ).toEqual(['a', 'b', 'hidden']);
    expect(
      reduceSelection(
        initial,
        { type: 'deselectByMask', matchingEntryIds: ids('a', 'b') },
        ids('a', 'b'),
      ).selectedEntryIds,
    ).toEqual(['hidden']);
  });

  it('restores a previous selection, filtered to entries still visible', () => {
    const initial: SelectionState = { selectedEntryIds: ['a'], cursorEntryId: 'a' };
    expect(
      reduceSelection(
        initial,
        { type: 'restore', entryIds: ids('a', 'b', 'gone') },
        ids('a', 'b', 'c'),
      ).selectedEntryIds,
    ).toEqual(['a', 'b']);
  });

  it('restore replaces the selection wholesale rather than merging with the current one', () => {
    const initial: SelectionState = { selectedEntryIds: ['c'], cursorEntryId: 'c' };
    expect(
      reduceSelection(initial, { type: 'restore', entryIds: ids('a') }, ids('a', 'b', 'c'))
        .selectedEntryIds,
    ).toEqual(['a']);
  });

  it('clears selection without moving the cursor', () => {
    expect(
      reduceSelection(
        { selectedEntryIds: ['a'], cursorEntryId: 'a', anchorEntryId: 'a' },
        { type: 'clear' },
        ids('a'),
      ),
    ).toEqual({ selectedEntryIds: [], cursorEntryId: 'a' });
  });

  it('pruning only removes the targeted ids, regardless of what is currently visible', () => {
    const state: SelectionState = {
      selectedEntryIds: ['hidden', 'visible', 'removed'],
      cursorEntryId: 'removed',
      anchorEntryId: 'hidden',
    };
    expect(
      reduceSelection(state, { type: 'prune', removedEntryIds: ids('removed') }, ids('visible')),
    ).toEqual({
      selectedEntryIds: ['hidden', 'visible'],
      anchorEntryId: 'hidden',
    });
  });

  it('moves the cursor to the preceding entry when its entry is pruned', () => {
    expect(
      reduceSelection(
        { selectedEntryIds: ['b'], cursorEntryId: 'b', anchorEntryId: 'b' },
        { type: 'prune', removedEntryIds: ids('b') },
        ids('a', 'b', 'c'),
      ),
    ).toEqual({
      selectedEntryIds: [],
      cursorEntryId: 'a',
      anchorEntryId: 'a',
    });
  });

  it('moving the cursor drops a multi-selection that is only partially visible', () => {
    const state: SelectionState = {
      selectedEntryIds: ['hidden', 'visible', 'removed'],
      cursorEntryId: 'removed',
      anchorEntryId: 'hidden',
    };
    expect(reduceSelection(state, { type: 'moveCursor', offset: 1 }, ids('visible'))).toEqual({
      selectedEntryIds: [],
      cursorEntryId: 'visible',
      anchorEntryId: 'visible',
    });
  });

  it('keeps an existing multi-selection when moving the cursor without Shift', () => {
    const state: SelectionState = {
      selectedEntryIds: ['a', 'b', 'c'],
      cursorEntryId: 'c',
      anchorEntryId: 'a',
    };
    expect(
      reduceSelection(state, { type: 'moveCursor', offset: 1 }, ids('a', 'b', 'c', 'd')),
    ).toEqual({
      selectedEntryIds: ['a', 'b', 'c'],
      cursorEntryId: 'd',
      anchorEntryId: 'd',
      baseSelectedEntryIds: ['a', 'b', 'c'],
    });
  });

  it('extends selection from preserved base after plain cursor move', () => {
    const initial: SelectionState = {
      selectedEntryIds: ['a', 'b', 'c'],
      cursorEntryId: 'c',
      anchorEntryId: 'a',
    };
    const afterMove = reduceSelection(
      initial,
      { type: 'moveCursor', offset: 1 },
      ids('a', 'b', 'c', 'd', 'e'),
    );
    expect(afterMove.selectedEntryIds).toEqual(['a', 'b', 'c']);
    expect(afterMove.cursorEntryId).toBe('d');

    // Shift+Down from 'd': unions base {a,b,c} with the departed row d.
    const extended = reduceSelection(
      afterMove,
      { type: 'extendRange', offset: 1 },
      ids('a', 'b', 'c', 'd', 'e'),
    );
    expect(extended.selectedEntryIds).toEqual(['a', 'b', 'c', 'd']);

    // Shift+Up returns the cursor to d; the anchor row remains selected with the base.
    const shrunk = reduceSelection(
      extended,
      { type: 'extendRange', offset: -1 },
      ids('a', 'b', 'c', 'd', 'e'),
    );
    expect(shrunk.selectedEntryIds).toEqual(['a', 'b', 'c', 'd']);
  });
});

function makeEntries(...specs: { id: string; locationUri?: string }[]): EntrySummary[] {
  return specs.map((s) => ({
    id: s.id as EntryId,
    location: s.locationUri
      ? { providerId: 'local' as never, uri: s.locationUri }
      : ({ providerId: 'local' as never, uri: `/entries/${s.id}` } as Location),
    name: s.id,
    kind: 'file',
    hidden: false,
    readOnly: false,
    metadataRevision: 0,
  }));
}

describe('getSelectedEntries', () => {
  it('returns empty array when selection is undefined', () => {
    const entries = makeEntries({ id: 'a' }, { id: 'b' });
    expect(getSelectedEntries(undefined, entries)).toEqual([]);
  });

  it('returns empty array when selection has no items', () => {
    expect(getSelectedEntries(emptySelection, makeEntries({ id: 'a' }))).toEqual([]);
  });

  it('returns matching entries for single selection', () => {
    const selection: SelectionState = { selectedEntryIds: ['b'] };
    const entries = makeEntries({ id: 'a' }, { id: 'b' }, { id: 'c' });
    expect(getSelectedEntries(selection, entries)).toEqual([entries[1]]);
  });

  it('returns matching entries for discontinuous multi-selection', () => {
    const selection: SelectionState = { selectedEntryIds: ['a', 'c'] };
    const entries = makeEntries({ id: 'a' }, { id: 'b' }, { id: 'c' });
    expect(getSelectedEntries(selection, entries)).toEqual([entries[0], entries[2]]);
  });

  it('returns empty array when selected ids have no overlap with entries', () => {
    const selection: SelectionState = { selectedEntryIds: ['x', 'y'] };
    const entries = makeEntries({ id: 'a' }, { id: 'b' });
    expect(getSelectedEntries(selection, entries)).toEqual([]);
  });

  it('returns empty array when entries list is empty', () => {
    const selection: SelectionState = { selectedEntryIds: ['a'] };
    expect(getSelectedEntries(selection, [])).toEqual([]);
  });

  it('preserves entry order from the directory listing, not selection order', () => {
    const selection: SelectionState = { selectedEntryIds: ['c', 'a'] };
    const entries = makeEntries({ id: 'a' }, { id: 'b' }, { id: 'c' });
    // Selection says ['c','a'] but directory order is ['a','b','c'], so result is ['a','c']
    expect(getSelectedEntries(selection, entries)).toEqual([entries[0], entries[2]]);
  });
});

describe('getSelectedEntryLocations', () => {
  it('returns empty array when selection is undefined', () => {
    const entries = makeEntries({ id: 'a' });
    expect(getSelectedEntryLocations(undefined, entries)).toEqual([]);
  });

  it('returns locations for matching entries', () => {
    const selection: SelectionState = { selectedEntryIds: ['b', 'c'] };
    const entries = makeEntries(
      { id: 'a', locationUri: '/dir/a' },
      { id: 'b', locationUri: '/dir/b' },
      { id: 'c', locationUri: '/dir/c' },
    );
    const locations = getSelectedEntryLocations(selection, entries);
    expect(locations).toEqual(entries.slice(1, 3).map((entry) => entry.location));
  });

  it('is equivalent to getSelectedEntries().map(entry => entry.location)', () => {
    const selection: SelectionState = { selectedEntryIds: ['a', 'c'] };
    const entries = makeEntries({ id: 'a' }, { id: 'b' }, { id: 'c' });
    const locations = getSelectedEntryLocations(selection, entries);
    const expected = getSelectedEntries(selection, entries).map((e) => e.location);
    expect(locations).toEqual(expected);
  });
});
