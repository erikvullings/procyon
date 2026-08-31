import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { ArchivePasswordDialog } from './archive-password-dialog';

let root: HTMLElement;

beforeEach(() => {
  root = document.createElement('div');
  document.body.appendChild(root);
});

afterEach(() => {
  m.mount(root, null);
  root.remove();
});

describe('ArchivePasswordDialog', () => {
  it('uses a masked field and submits without rendering the password as text', () => {
    const confirm = vi.fn();
    m.mount(root, {
      view: () =>
        m(ArchivePasswordDialog, {
          open: true,
          invalid: false,
          archiveLabel: 'secret.zip',
          onConfirm: confirm,
          onCancel: vi.fn(),
        }),
    });
    m.redraw.sync();
    const input = root.querySelector<HTMLInputElement>('#archive-password');
    expect(input?.type).toBe('password');
    if (!input) throw new Error('password input missing');
    input.value = 'top-secret';
    input.dispatchEvent(new InputEvent('input', { bubbles: true }));
    expect(root.textContent).not.toContain('top-secret');
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    expect(confirm).toHaveBeenCalledWith('top-secret');
  });
});
