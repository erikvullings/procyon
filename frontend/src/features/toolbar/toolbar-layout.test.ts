import { describe, expect, it } from 'vitest';
import {
  addToolbarAction,
  DEFAULT_TOOLBAR_LAYOUT,
  moveToolbarItem,
  readToolbarLayout,
  removeToolbarItem,
  resetToolbarLayout,
  setToolbarLabelMode,
  TOOLBAR_LAYOUT_VERSION,
  type ToolbarLayout,
  toolbarLayoutsEqual,
} from './toolbar-layout.ts';

const sample: ToolbarLayout = {
  version: TOOLBAR_LAYOUT_VERSION,
  items: [
    { kind: 'action', actionId: 'core.parent', labels: 'icon' },
    { kind: 'separator' },
    { kind: 'action', actionId: 'core.palette', labels: 'icon-text' },
  ],
};

describe('default toolbar layout', () => {
  it('starts with navigation, then search, then the workspace-level controls', () => {
    expect(DEFAULT_TOOLBAR_LAYOUT.items.slice(0, 3)).toEqual([
      { kind: 'action', actionId: 'core.parent', labels: 'icon' },
      { kind: 'separator' },
      { kind: 'action', actionId: 'core.findFiles', labels: 'icon' },
    ]);
  });

  it('lists each action at most once', () => {
    const ids = DEFAULT_TOOLBAR_LAYOUT.items.flatMap((item) =>
      item.kind === 'action' ? [item.actionId] : [],
    );
    expect(new Set(ids).size).toBe(ids.length);
  });
});

describe('readToolbarLayout', () => {
  it('returns a current, well-formed document unchanged', () => {
    const read = readToolbarLayout(sample);
    expect(read).toEqual({ layout: sample, newerVersion: false });
  });

  it('falls back to the default for missing or malformed input', () => {
    expect(readToolbarLayout(undefined)).toEqual({
      layout: DEFAULT_TOOLBAR_LAYOUT,
      newerVersion: false,
    });
    expect(readToolbarLayout('nonsense').layout).toEqual(DEFAULT_TOOLBAR_LAYOUT);
    expect(readToolbarLayout({ version: 1, items: 'x' }).layout).toEqual(DEFAULT_TOOLBAR_LAYOUT);
  });

  it('drops malformed items and duplicate actions, keeping the first occurrence', () => {
    const read = readToolbarLayout({
      version: 1,
      items: [
        { kind: 'action', actionId: 'core.parent', labels: 'icon' },
        { kind: 'action', actionId: 'core.parent', labels: 'icon-text' },
        { kind: 'action', actionId: '', labels: 'icon' },
        { kind: 'action', actionId: 'core.copy', labels: 'huge' },
        { kind: 'mystery' },
        { kind: 'separator' },
      ],
    });
    expect(read.layout.items).toEqual([
      { kind: 'action', actionId: 'core.parent', labels: 'icon' },
      { kind: 'separator' },
    ]);
  });

  it('flags a document written by a newer version so callers do not overwrite it', () => {
    const read = readToolbarLayout({ version: TOOLBAR_LAYOUT_VERSION + 1, items: [] });
    expect(read.newerVersion).toBe(true);
    expect(read.layout).toEqual(DEFAULT_TOOLBAR_LAYOUT);
  });
});

describe('editing the layout', () => {
  it('adds an action at the end, or at a given index', () => {
    const appended = addToolbarAction(sample, 'core.copy');
    expect(appended.items.at(-1)).toEqual({
      kind: 'action',
      actionId: 'core.copy',
      labels: 'icon',
    });
    const inserted = addToolbarAction(sample, 'core.copy', 0);
    expect(inserted.items[0]).toEqual({ kind: 'action', actionId: 'core.copy', labels: 'icon' });
  });

  it('clamps an out-of-range insertion index instead of splicing from the end', () => {
    const inserted = addToolbarAction(sample, 'core.copy', -1);
    expect(inserted.items[0]).toEqual({ kind: 'action', actionId: 'core.copy', labels: 'icon' });
    expect(addToolbarAction(sample, 'core.copy', 99).items.at(-1)).toEqual({
      kind: 'action',
      actionId: 'core.copy',
      labels: 'icon',
    });
  });

  it('does not add an action that is already placed', () => {
    expect(addToolbarAction(sample, 'core.parent')).toBe(sample);
  });

  it('removes the item at an index', () => {
    expect(removeToolbarItem(sample, 1).items).toEqual([
      { kind: 'action', actionId: 'core.parent', labels: 'icon' },
      { kind: 'action', actionId: 'core.palette', labels: 'icon-text' },
    ]);
  });

  it('moves an item from one index to another', () => {
    expect(moveToolbarItem(sample, 2, 0).items.map((item) => item.kind)).toEqual([
      'action',
      'action',
      'separator',
    ]);
    expect(moveToolbarItem(sample, 0, 0)).toBe(sample);
  });

  it('sets the label mode of an action and ignores separators', () => {
    const iconOnly = setToolbarLabelMode(sample, 2, 'icon');
    expect(iconOnly.items[2]).toEqual({ kind: 'action', actionId: 'core.palette', labels: 'icon' });
    expect(setToolbarLabelMode(sample, 1, 'icon-text')).toBe(sample);
  });

  it('never mutates the layout it was given', () => {
    const snapshot = structuredClone(sample);
    addToolbarAction(sample, 'core.copy');
    removeToolbarItem(sample, 0);
    moveToolbarItem(sample, 0, 2);
    setToolbarLabelMode(sample, 0, 'icon-text');
    expect(sample).toEqual(snapshot);
  });
});

describe('reset and equality', () => {
  it('reset restores the default layout exactly', () => {
    const reset = resetToolbarLayout();
    expect(toolbarLayoutsEqual(reset, DEFAULT_TOOLBAR_LAYOUT)).toBe(true);
    expect(toolbarLayoutsEqual(sample, DEFAULT_TOOLBAR_LAYOUT)).toBe(false);
  });
});
