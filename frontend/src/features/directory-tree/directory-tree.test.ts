import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { EntrySummary, Location } from '../../models';
import { DirectoryTree, type DirectoryTreeAttrs } from './directory-tree';
import { createTreeChildrenState, withChildren } from './directory-tree-state';

let root: HTMLElement;

function location(uri: string): Location {
  return { providerId: 'local', uri };
}

function directoryEntry(uri: string, name: string): EntrySummary {
  return {
    id: uri,
    location: location(uri),
    name,
    kind: 'directory',
    hidden: false,
    readOnly: false,
    metadataRevision: 1,
  };
}

function mount(attrs: DirectoryTreeAttrs): void {
  m.mount(root, { view: () => m(DirectoryTree, attrs) });
}

beforeEach(() => {
  root = document.createElement('div');
  document.body.appendChild(root);
});

afterEach(() => {
  m.mount(root, null);
  root.remove();
});

const TREE_ROOT = { location: location('file:///'), name: 'root' };

describe('DirectoryTree', () => {
  it('renders only the root row when nothing is expanded', () => {
    mount({
      root: TREE_ROOT,
      state: createTreeChildrenState(),
      onToggleExpand: vi.fn(),
      onActivate: vi.fn(),
      viewportHeight: 200,
    });

    const rows = root.querySelectorAll('.fm-tree-row');
    expect(rows).toHaveLength(1);
    expect(rows[0]?.textContent).toContain('root');
  });

  it('clicking the expand toggle on an unfetched child node requests its children (lazy expansion)', () => {
    const onToggleExpand = vi.fn();
    const state = withChildren(createTreeChildrenState(), 'file:///', [
      directoryEntry('file:///alpha', 'alpha'),
    ]);
    mount({
      root: TREE_ROOT,
      state,
      onToggleExpand,
      onActivate: vi.fn(),
      viewportHeight: 200,
    });

    root.querySelector<HTMLButtonElement>('.fm-tree-expand-toggle')?.click();

    expect(onToggleExpand).toHaveBeenCalledWith(location('file:///alpha'));
  });

  it('renders no expand-toggle affordance for the root row (task 0139 follow-up)', () => {
    mount({
      root: TREE_ROOT,
      state: createTreeChildrenState(),
      onToggleExpand: vi.fn(),
      onActivate: vi.fn(),
      viewportHeight: 200,
    });

    expect(root.querySelector('.fm-tree-expand-toggle')).toBeNull();
  });

  it('shows cached children once expanded, indented one level deeper', () => {
    const state = withChildren(createTreeChildrenState(), 'file:///', [
      directoryEntry('file:///alpha', 'alpha'),
    ]);
    mount({
      root: TREE_ROOT,
      state,
      onToggleExpand: vi.fn(),
      onActivate: vi.fn(),
      viewportHeight: 200,
    });

    const rows = root.querySelectorAll('.fm-tree-row');
    expect(rows).toHaveLength(2);
    expect(rows[1]?.textContent).toContain('alpha');
    expect(rows[1]?.getAttribute('aria-level')).toBe('2');
  });

  it('marks the row matching the active pane location as selected', () => {
    const state = withChildren(createTreeChildrenState(), 'file:///', [
      directoryEntry('file:///alpha', 'alpha'),
      directoryEntry('file:///zeta', 'zeta'),
    ]);
    mount({
      root: TREE_ROOT,
      state,
      activeLocationUri: 'file:///zeta',
      onToggleExpand: vi.fn(),
      onActivate: vi.fn(),
      viewportHeight: 200,
    });

    const selected = root.querySelector('[aria-selected="true"]');
    expect(selected?.textContent).toContain('zeta');
  });

  it('clicking a row activates (navigates to) its location', () => {
    const onActivate = vi.fn();
    const state = withChildren(createTreeChildrenState(), 'file:///', [
      directoryEntry('file:///alpha', 'alpha'),
    ]);
    mount({
      root: TREE_ROOT,
      state,
      onToggleExpand: vi.fn(),
      onActivate,
      viewportHeight: 200,
    });

    const rows = root.querySelectorAll<HTMLElement>('.fm-tree-row');
    rows[1]?.click();

    expect(onActivate).toHaveBeenCalledWith(location('file:///alpha'));
  });

  it('ArrowDown moves aria-activedescendant to the next row', () => {
    const state = withChildren(createTreeChildrenState(), 'file:///', [
      directoryEntry('file:///alpha', 'alpha'),
    ]);
    mount({
      root: TREE_ROOT,
      state,
      activeLocationUri: 'file:///',
      onToggleExpand: vi.fn(),
      onActivate: vi.fn(),
      viewportHeight: 200,
    });

    const tree = root.querySelector<HTMLElement>('.fm-directory-tree');
    tree?.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }));
    m.redraw.sync();

    const active = tree?.getAttribute('aria-activedescendant');
    expect(active).toContain(encodeURIComponent('file:///alpha'));
  });

  it('ArrowRight on a collapsed unfetched node requests expansion', () => {
    const onToggleExpand = vi.fn();
    mount({
      root: TREE_ROOT,
      state: createTreeChildrenState(),
      activeLocationUri: 'file:///',
      onToggleExpand,
      onActivate: vi.fn(),
      viewportHeight: 200,
    });

    const tree = root.querySelector<HTMLElement>('.fm-directory-tree');
    tree?.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true }));

    expect(onToggleExpand).toHaveBeenCalledWith(location('file:///'));
  });

  it('Enter on the focused row activates it', () => {
    const onActivate = vi.fn();
    mount({
      root: TREE_ROOT,
      state: createTreeChildrenState(),
      activeLocationUri: 'file:///',
      onToggleExpand: vi.fn(),
      onActivate,
      viewportHeight: 200,
    });

    const tree = root.querySelector<HTMLElement>('.fm-directory-tree');
    tree?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));

    expect(onActivate).toHaveBeenCalledWith(location('file:///'));
  });

  it('windows a large expanded child list rather than rendering every row', () => {
    const many = Array.from({ length: 500 }, (_, index) =>
      directoryEntry(`file:///child-${index}`, `child-${index}`),
    );
    const state = withChildren(createTreeChildrenState(), 'file:///', many);
    mount({
      root: TREE_ROOT,
      state,
      onToggleExpand: vi.fn(),
      onActivate: vi.fn(),
      viewportHeight: 200,
    });

    const rendered = root.querySelectorAll('.fm-tree-row').length;
    expect(rendered).toBeGreaterThan(1);
    expect(rendered).toBeLessThan(501);
  });

  it('shows a loading indicator for a child node whose fetch is in flight', () => {
    let state = withChildren(createTreeChildrenState(), 'file:///', [
      directoryEntry('file:///alpha', 'alpha'),
    ]);
    // Simulate an in-flight fetch via the loading-uri path exercised by directory-tree-state.
    state = { ...state, loadingUris: new Set(['file:///alpha']) };
    mount({
      root: TREE_ROOT,
      state,
      onToggleExpand: vi.fn(),
      onActivate: vi.fn(),
      viewportHeight: 200,
    });

    expect(root.querySelector('.fm-tree-expand-toggle .fm-tree-loading-spinner')).not.toBeNull();
  });

  it('PageDown moves the cursor by a page, clamped to the last row, not just scrolling', () => {
    const state = withChildren(
      createTreeChildrenState(),
      'file:///',
      Array.from({ length: 3 }, (_, index) => directoryEntry(`file:///c${index}`, `c${index}`)),
    );
    mount({
      root: TREE_ROOT,
      state,
      activeLocationUri: 'file:///',
      onToggleExpand: vi.fn(),
      onActivate: vi.fn(),
      viewportHeight: 200,
    });

    const tree = root.querySelector<HTMLElement>('.fm-directory-tree');
    tree?.dispatchEvent(new KeyboardEvent('keydown', { key: 'PageDown', bubbles: true }));
    m.redraw.sync();

    // Only 4 rows total (root + 3 children); a 10-row page jump clamps to the last one instead
    // of doing nothing.
    expect(tree?.getAttribute('aria-activedescendant')).toContain(encodeURIComponent('file:///c2'));
  });

  it('Tab/Shift+Tab calls onTabOut with the corresponding direction instead of scrolling focus', () => {
    const onTabOut = vi.fn();
    mount({
      root: TREE_ROOT,
      state: createTreeChildrenState(),
      onToggleExpand: vi.fn(),
      onActivate: vi.fn(),
      onTabOut,
      viewportHeight: 200,
    });

    const tree = root.querySelector<HTMLElement>('.fm-directory-tree');
    tree?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab', bubbles: true }));
    tree?.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'Tab', shiftKey: true, bubbles: true }),
    );

    expect(onTabOut).toHaveBeenNthCalledWith(1, 1);
    expect(onTabOut).toHaveBeenNthCalledWith(2, -1);
  });

  it('registerFocus lets the caller move DOM focus into the tree', () => {
    let focus: (() => boolean) | undefined;
    mount({
      root: TREE_ROOT,
      state: createTreeChildrenState(),
      onToggleExpand: vi.fn(),
      onActivate: vi.fn(),
      registerFocus: (callback) => {
        focus = callback;
      },
      viewportHeight: 200,
    });

    expect(document.activeElement).not.toBe(root.querySelector('.fm-directory-tree'));
    expect(focus?.()).toBe(true);
    expect(document.activeElement).toBe(root.querySelector('.fm-directory-tree'));
  });
});
