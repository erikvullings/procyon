import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { SyncPlanItem } from '../../models';
import { SyncPlanDialog } from './sync-plan-dialog';

let root: HTMLElement;

beforeEach(() => {
  root = document.createElement('div');
  document.body.appendChild(root);
});

afterEach(() => {
  m.mount(root, null);
  root.remove();
});

function findButton(label: string): HTMLButtonElement {
  const button = [...document.querySelectorAll('button')].find((candidate) =>
    candidate.textContent?.includes(label),
  );
  if (button === undefined) throw new Error(`no button labelled "${label}"`);
  return button;
}

const ITEMS: SyncPlanItem[] = [
  {
    relativePath: 'only-left.txt',
    status: 'onlyLeft',
    action: 'copyLeftToRight',
    left: { kind: 'file', size: 10 },
  },
  {
    relativePath: 'only-right.txt',
    status: 'onlyRight',
    action: 'deleteRight',
    right: { kind: 'file', size: 20 },
  },
];

describe('SyncPlanDialog', () => {
  it('renders one row per plan item with its path, status and size summary', () => {
    m.mount(root, {
      view: () =>
        m(SyncPlanDialog, { open: true, items: ITEMS, onApply: vi.fn(), onCancel: vi.fn() }),
    });
    m.redraw.sync();

    const rows = [...document.querySelectorAll('.fm-sync-plan-table tbody tr')];
    expect(rows).toHaveLength(2);
    expect(rows[0]?.textContent).toContain('only-left.txt');
    expect(rows[0]?.textContent).toContain('Only left');
    expect(rows[1]?.textContent).toContain('only-right.txt');
    expect(rows[1]?.textContent).toContain('Only right');
  });

  it('shows a no-op message instead of a table when the plan is empty', () => {
    m.mount(root, {
      view: () => m(SyncPlanDialog, { open: true, items: [], onApply: vi.fn(), onCancel: vi.fn() }),
    });
    m.redraw.sync();

    expect(document.querySelector('.fm-sync-plan-table')).toBeNull();
    expect(root.textContent).toContain('nothing to synchronize');
  });

  it('applies the plan exactly as shown when Apply is clicked', () => {
    const onApply = vi.fn();
    m.mount(root, {
      view: () => m(SyncPlanDialog, { open: true, items: ITEMS, onApply, onCancel: vi.fn() }),
    });
    m.redraw.sync();

    findButton('Apply').dispatchEvent(new MouseEvent('click', { bubbles: true }));

    expect(onApply).toHaveBeenCalledWith(ITEMS);
  });

  it('disables Apply when every row is skipped', () => {
    const skippedOnly: SyncPlanItem[] = ITEMS.map((item) => ({ ...item, action: 'skip' }));
    m.mount(root, {
      view: () =>
        m(SyncPlanDialog, {
          open: true,
          items: skippedOnly,
          onApply: vi.fn(),
          onCancel: vi.fn(),
        }),
    });
    m.redraw.sync();

    expect(findButton('Apply').disabled).toBe(true);
  });

  it('shows the actionable count in the Apply label', () => {
    m.mount(root, {
      view: () =>
        m(SyncPlanDialog, { open: true, items: ITEMS, onApply: vi.fn(), onCancel: vi.fn() }),
    });
    m.redraw.sync();

    expect(findButton('Apply').textContent).toContain('2');
  });

  it('cancels without applying', () => {
    const onApply = vi.fn();
    const onCancel = vi.fn();
    m.mount(root, {
      view: () => m(SyncPlanDialog, { open: true, items: ITEMS, onApply, onCancel }),
    });
    m.redraw.sync();

    findButton('Cancel').dispatchEvent(new MouseEvent('click', { bubbles: true }));

    expect(onCancel).toHaveBeenCalledOnce();
    expect(onApply).not.toHaveBeenCalled();
  });

  it('shows a backend error message when applying failed', () => {
    m.mount(root, {
      view: () =>
        m(SyncPlanDialog, {
          open: true,
          items: ITEMS,
          error: 'Unable to apply the plan',
          onApply: vi.fn(),
          onCancel: vi.fn(),
        }),
    });
    m.redraw.sync();

    expect(document.querySelector('.fm-field-error')?.textContent).toBe('Unable to apply the plan');
  });

  it('disables Apply and shows a busy label while applying', () => {
    m.mount(root, {
      view: () =>
        m(SyncPlanDialog, {
          open: true,
          items: ITEMS,
          applying: true,
          onApply: vi.fn(),
          onCancel: vi.fn(),
        }),
    });
    m.redraw.sync();

    const button = findButton('Applying');
    expect(button.disabled).toBe(true);
  });

  it('resets to the freshly supplied items each time the dialog reopens', () => {
    const onApply = vi.fn();
    let open = true;
    let items = ITEMS;
    m.mount(root, {
      view: () => m(SyncPlanDialog, { open, items, onApply, onCancel: vi.fn() }),
    });
    m.redraw.sync();

    open = false;
    m.redraw.sync();
    items = [ITEMS[0] as SyncPlanItem];
    open = true;
    m.redraw.sync();

    findButton('Apply').dispatchEvent(new MouseEvent('click', { bubbles: true }));
    expect(onApply).toHaveBeenCalledWith([ITEMS[0]]);
  });
});
