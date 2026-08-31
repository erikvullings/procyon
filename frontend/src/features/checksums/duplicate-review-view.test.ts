import m from 'mithril';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { DuplicateGroup, Location } from '../../models';
import { DuplicateReviewView, type DuplicateReviewViewAttrs } from './duplicate-review-view';

const mounted: HTMLElement[] = [];

afterEach(() => {
  // Rendered with `m.render`, so tear down with `m.render(el, [])` rather
  // than `m.mount(el, null)`: mixing the two leaves Mithril's per-element
  // renderer state inconsistent and silently blanks later renders.
  for (const element of mounted) {
    m.render(element, []);
    element.remove();
  }
  mounted.length = 0;
});

function location(uri: string): Location {
  return { providerId: 'local', uri };
}

/**
 * Mounts the view on its own root. Each call gets a fresh root so a test that
 * renders twice (to compare two attribute sets) never re-renders over a
 * previous mount.
 */
function render(attrs: Partial<DuplicateReviewViewAttrs> = {}): HTMLElement {
  const root = document.createElement('div');
  document.body.appendChild(root);
  mounted.push(root);
  const full: DuplicateReviewViewAttrs = {
    groups: [],
    isComplete: true,
    isCancelled: false,
    warningsCount: 0,
    selectedUris: new Set(),
    totalReclaimableBytes: 0,
    isLastCopy: () => false,
    onToggle: vi.fn(),
    onDeleteSelected: vi.fn(),
    onCancel: vi.fn(),
    onClose: vi.fn(),
    ...attrs,
  };
  m.render(root, m(DuplicateReviewView, full));
  return root;
}

const duplicateGroup: DuplicateGroup = {
  fullHash: 'aaa',
  size: 2048,
  hardlinkClusters: [],
  distinctLocations: [location('file:///root/a.bin'), location('file:///root/b.bin')],
  reclaimableBytes: 2048,
};

const hardlinkGroup: DuplicateGroup = {
  fullHash: 'bbb',
  size: 4096,
  hardlinkClusters: [
    { device: 1, inode: 2, locations: [location('file:///root/x'), location('file:///root/y')] },
  ],
  distinctLocations: [],
  reclaimableBytes: 0,
};

describe('DuplicateReviewView', () => {
  it('reports when nothing was found', () => {
    const root = render();
    expect(root.querySelector('.duplicate-review__empty')?.textContent).toMatch(/no duplicate/i);
  });

  it('lists each copy in a duplicate group with a checkbox', () => {
    const root = render({ groups: [duplicateGroup], totalReclaimableBytes: 2048 });
    const checkboxes = root.querySelectorAll('.duplicate-review__copies input[type=checkbox]');
    expect(checkboxes).toHaveLength(2);
    expect(root.textContent).toContain('a.bin');
    expect(root.textContent).toContain('b.bin');
  });

  it('renders a hardlink cluster distinctly with an explanatory note', () => {
    const root = render({ groups: [hardlinkGroup] });
    const note = root.querySelector('.duplicate-review__hardlink-note');
    expect(note?.textContent).toMatch(/one file/i);
    expect(note?.textContent).toMatch(/frees no space/i);
    // The hardlinked paths are not listed as ordinary distinct duplicates.
    expect(root.querySelectorAll('.duplicate-review__hardlinks')).toHaveLength(1);
  });

  it('disables the last surviving copy so every copy cannot be deleted', () => {
    const root = render({
      groups: [duplicateGroup],
      selectedUris: new Set(['file:///root/a.bin']),
      isLastCopy: (uri) => uri === 'file:///root/b.bin',
    });
    const checkboxes = [
      ...root.querySelectorAll<HTMLInputElement>('.duplicate-review__copies input[type=checkbox]'),
    ];
    expect(checkboxes[0]?.checked).toBe(true);
    expect(checkboxes[1]?.disabled).toBe(true);
    expect(root.textContent).toMatch(/last remaining copy/i);
  });

  it('enables deletion only once something is ticked', () => {
    const idle = render({ groups: [duplicateGroup] });
    expect(idle.querySelector<HTMLButtonElement>('.duplicate-review__delete')?.disabled).toBe(true);

    const ticked = render({
      groups: [duplicateGroup],
      selectedUris: new Set(['file:///root/a.bin']),
    });
    const button = ticked.querySelector<HTMLButtonElement>('.duplicate-review__delete');
    expect(button?.disabled).toBe(false);
    expect(button?.textContent).toContain('1 selected');
  });

  it('invokes the delete handler, which owns the confirmation flow', () => {
    const onDeleteSelected = vi.fn();
    const root = render({
      groups: [duplicateGroup],
      selectedUris: new Set(['file:///root/a.bin']),
      onDeleteSelected,
    });
    root.querySelector<HTMLButtonElement>('.duplicate-review__delete')?.click();
    expect(onDeleteSelected).toHaveBeenCalledOnce();
  });

  it('says so when the scan was cancelled rather than implying completeness', () => {
    const root = render({ isCancelled: true, isComplete: true });
    expect(root.querySelector('.duplicate-review__summary')?.textContent).toMatch(
      /cancelled — results are incomplete/i,
    );
  });

  it('surfaces skipped files as a warning', () => {
    const root = render({ warningsCount: 3 });
    expect(root.querySelector('.duplicate-review__warnings')?.textContent).toContain('3 file(s)');
  });
});
