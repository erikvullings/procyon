import { describe, expect, it } from 'vitest';

import type { EntrySummary } from '../../models';
import { CORE_SELECTION_ACTION_IDS, interpretSelectionKey, reduceTypeahead } from './keybindings';

function key(
  value: string,
  modifiers: Partial<{
    shiftKey: boolean;
    ctrlKey: boolean;
    metaKey: boolean;
    altKey: boolean;
  }> = {},
) {
  return {
    key: value,
    shiftKey: false,
    ctrlKey: false,
    metaKey: false,
    altKey: false,
    ...modifiers,
  };
}

const entries: readonly EntrySummary[] = [
  {
    id: 'alpha',
    location: { providerId: 'file', uri: 'mock:///Alpha.txt' },
    name: 'Alpha.txt',
    kind: 'file',
    hidden: false,
    readOnly: false,
    metadataRevision: 1,
  },
  {
    id: 'archive',
    location: { providerId: 'file', uri: 'mock:///archive.zip' },
    name: 'archive.zip',
    kind: 'file',
    hidden: false,
    readOnly: false,
    metadataRevision: 1,
  },
  {
    id: 'beta',
    location: { providerId: 'file', uri: 'mock:///Beta.txt' },
    name: 'Beta.txt',
    kind: 'file',
    hidden: false,
    readOnly: false,
    metadataRevision: 1,
  },
];

describe('selection keybindings', () => {
  it.each([
    ['ArrowUp', { type: 'moveCursor', offset: -1 }],
    ['ArrowDown', { type: 'moveCursor', offset: 1 }],
    ['PageUp', { type: 'moveCursorByPage', pages: -1 }],
    ['PageDown', { type: 'moveCursorByPage', pages: 1 }],
    ['Home', { type: 'moveCursorTo', edge: 'first' }],
    ['End', { type: 'moveCursorTo', edge: 'last' }],
    ['Enter', { type: 'open' }],
    ['Backspace', { type: 'parent' }],
    ['Tab', { type: 'switchPane', direction: 1 }],
    [' ', { type: 'toggleCursorSelection' }],
  ] as const)('maps %s to its semantic command', (pressed, command) => {
    expect(interpretSelectionKey(key(pressed), 'linux')).toEqual(command);
  });

  it('reverses pane switching for Shift+Tab', () => {
    expect(interpretSelectionKey(key('Tab', { shiftKey: true }), 'linux')).toEqual({
      type: 'switchPane',
      direction: -1,
    });
  });

  it.each([
    ['ArrowUp', -1],
    ['ArrowDown', 1],
  ] as const)('extends selection for Shift+%s', (pressed, offset) => {
    expect(interpretSelectionKey(key(pressed, { shiftKey: true }), 'linux')).toEqual({
      type: 'extendRange',
      offset,
    });
  });

  it('uses Command+A only on macOS and Control+A on Windows/Linux', () => {
    expect(interpretSelectionKey(key('a', { metaKey: true }), 'macos')).toEqual({
      type: 'selectAll',
    });
    expect(interpretSelectionKey(key('a', { ctrlKey: true }), 'macos')).toBeUndefined();
    expect(interpretSelectionKey(key('a', { ctrlKey: true }), 'windows')).toEqual({
      type: 'selectAll',
    });
    expect(interpretSelectionKey(key('a', { metaKey: true }), 'linux')).toBeUndefined();
  });

  it('does not claim modified browser shortcuts that this feature does not own', () => {
    expect(interpretSelectionKey(key('r', { ctrlKey: true }), 'windows')).toBeUndefined();
    expect(interpretSelectionKey(key('p', { metaKey: true }), 'macos')).toBeUndefined();
  });

  it('defines stable action ids for future action-system routing', () => {
    expect(CORE_SELECTION_ACTION_IDS).toMatchObject({
      selectAll: 'core.selectAll',
      invertSelection: 'core.invertSelection',
      clearSelection: 'core.clearSelection',
      toggleSelection: 'core.toggleSelection',
    });
  });
});

describe('type-to-select', () => {
  it('jumps to the first case-insensitive in-word match', () => {
    expect(reduceTypeahead(undefined, 'lph', entries, 1_000)).toEqual({
      state: { prefix: 'lph', lastInputAt: 1_000 },
      matchedEntryId: 'alpha',
    });
  });

  it('jumps to the first case-insensitive match', () => {
    expect(reduceTypeahead(undefined, 'a', entries, 1_000)).toEqual({
      state: { prefix: 'a', lastInputAt: 1_000 },
      matchedEntryId: 'alpha',
    });
  });

  it('extends the prefix within the timeout', () => {
    expect(reduceTypeahead({ prefix: 'a', lastInputAt: 1_000 }, 'r', entries, 1_400)).toEqual({
      state: { prefix: 'ar', lastInputAt: 1_400 },
      matchedEntryId: 'archive',
    });
  });

  it('resets the prefix after the timeout', () => {
    expect(reduceTypeahead({ prefix: 'a', lastInputAt: 1_000 }, 'b', entries, 1_701)).toEqual({
      state: { prefix: 'b', lastInputAt: 1_701 },
      matchedEntryId: 'beta',
    });
  });
});
