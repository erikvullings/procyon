import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { PermanentDeleteDialog } from './permanent-delete-dialog';

let root: HTMLElement;
const formatSettings = { sizeFormat: 'binary', dateFormat: 'medium' } as const;

beforeEach(() => {
  root = document.createElement('div');
  document.body.appendChild(root);
});

afterEach(() => {
  m.mount(root, null);
  root.remove();
});

describe('PermanentDeleteDialog', () => {
  it('states exact totals and defaults focus to cancel', () => {
    m.mount(root, {
      view: () =>
        m(PermanentDeleteDialog, {
          open: true,
          operationId: 'delete-1',
          itemCount: 12,
          totalBytes: 4096,
          formatSettings,
          onConfirm: vi.fn(),
          onCancel: vi.fn(),
        }),
    });
    m.redraw.sync();

    expect(root.textContent).toContain('12 items (4 K)');
    expect(root.textContent).toContain('irreversible');
    expect(document.activeElement?.textContent).toBe('Cancel');
  });

  it('moves Tab from cancel to permanent delete', () => {
    m.mount(root, {
      view: () =>
        m(PermanentDeleteDialog, {
          open: true,
          operationId: 'delete-1',
          itemCount: 1,
          totalBytes: 10,
          formatSettings,
          onConfirm: vi.fn(),
          onCancel: vi.fn(),
        }),
    });
    m.redraw.sync();

    const cancel = root.querySelector<HTMLButtonElement>('.fm-permanent-delete-cancel');
    const confirm = root.querySelector<HTMLButtonElement>('.fm-permanent-delete-confirm');
    cancel?.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'Tab', bubbles: true, cancelable: true }),
    );
    expect(document.activeElement).toBe(confirm);
  });

  it('cycles Tab only between cancel and permanent delete', () => {
    m.mount(root, {
      view: () =>
        m(PermanentDeleteDialog, {
          open: true,
          operationId: 'delete-1',
          itemCount: 1,
          totalBytes: 10,
          formatSettings,
          onConfirm: vi.fn(),
          onCancel: vi.fn(),
        }),
    });
    m.redraw.sync();

    const cancel = root.querySelector<HTMLButtonElement>('.fm-permanent-delete-cancel');
    const confirm = root.querySelector<HTMLButtonElement>('.fm-permanent-delete-confirm');
    cancel?.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'Tab', bubbles: true, cancelable: true }),
    );
    confirm?.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'Tab', bubbles: true, cancelable: true }),
    );
    expect(document.activeElement).toBe(cancel);
  });

  it('closes immediately after permanent delete is confirmed', () => {
    const onConfirm = vi.fn();
    m.mount(root, {
      view: () =>
        m(PermanentDeleteDialog, {
          open: true,
          operationId: 'delete-1',
          itemCount: 1,
          totalBytes: 1024,
          formatSettings,
          onConfirm,
          onCancel: vi.fn(),
        }),
    });
    m.redraw.sync();

    root.querySelector<HTMLButtonElement>('.fm-permanent-delete-confirm')?.click();
    m.redraw.sync();

    expect(onConfirm).toHaveBeenCalledOnce();
    expect(root.querySelector('[role="dialog"]')?.getAttribute('aria-hidden')).toBe('true');
  });

  it('reopens when the backend rejects the confirmation', async () => {
    const onConfirm = vi.fn(() => Promise.reject(new Error('failed')));
    m.mount(root, {
      view: () =>
        m(PermanentDeleteDialog, {
          open: true,
          operationId: 'delete-1',
          itemCount: 1,
          totalBytes: 1024,
          formatSettings,
          onConfirm,
          onCancel: vi.fn(),
        }),
    });
    m.redraw.sync();

    root.querySelector<HTMLButtonElement>('.fm-permanent-delete-confirm')?.click();
    await Promise.resolve();
    m.redraw.sync();

    expect(root.querySelector('[role="dialog"]')?.getAttribute('aria-hidden')).toBe('false');
  });
});
