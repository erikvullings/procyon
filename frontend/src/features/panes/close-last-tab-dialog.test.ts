import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { CloseLastTabDialog } from './close-last-tab-dialog';

let root: HTMLElement;

beforeEach(() => {
  root = document.createElement('div');
  document.body.appendChild(root);
});

afterEach(() => {
  m.mount(root, null);
  root.remove();
});

describe('CloseLastTabDialog', () => {
  it('confirms before closing a pane down to zero tabs', () => {
    const onConfirm = vi.fn();
    const onCancel = vi.fn();
    m.mount(root, {
      view: () => m(CloseLastTabDialog, { open: true, onConfirm, onCancel }),
    });
    m.redraw.sync();

    expect(root.textContent).toContain('only tab');
    [...root.querySelectorAll('button')].find((b) => b.textContent === 'Close tab')?.click();
    expect(onConfirm).toHaveBeenCalledOnce();

    [...root.querySelectorAll('button')].find((b) => b.textContent === 'Cancel')?.click();
    expect(onCancel).toHaveBeenCalledOnce();
  });

  it('renders nothing interactive while closed', () => {
    m.mount(root, {
      view: () => m(CloseLastTabDialog, { open: false, onConfirm: vi.fn(), onCancel: vi.fn() }),
    });
    m.redraw.sync();

    const dialog = root.querySelector('[role="dialog"]');
    expect(dialog?.getAttribute('aria-hidden')).toBe('true');
  });
});
