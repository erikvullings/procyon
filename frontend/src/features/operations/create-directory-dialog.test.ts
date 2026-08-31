import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { setLocale } from '../../i18n';
import { CreateDirectoryDialog, validateDirectoryName } from './create-directory-dialog';

let root: HTMLElement;

beforeEach(() => {
  root = document.createElement('div');
  document.body.appendChild(root);
});

afterEach(() => {
  setLocale('en');
  m.mount(root, null);
  root.remove();
});

describe('CreateDirectoryDialog', () => {
  it('validates empty, traversal, invalid-character and reserved names', () => {
    expect(validateDirectoryName('')).toBe('Enter a folder name.');
    expect(validateDirectoryName('../escape')).toBe('Use a single folder name.');
    expect(validateDirectoryName('bad/name')).toBe('Use a single folder name.');
    expect(validateDirectoryName('bad\0name')).toBe('The name contains invalid characters.');
    expect(validateDirectoryName('COM1.txt')).toBe('That name is reserved by Windows.');
    expect(validateDirectoryName('資料')).toBeUndefined();
  });

  it('is focused and confirms with Enter while Escape cancels', async () => {
    const confirm = vi.fn();
    const cancel = vi.fn();
    m.mount(root, {
      view: () => m(CreateDirectoryDialog, { open: true, onConfirm: confirm, onCancel: cancel }),
    });
    m.redraw.sync();
    const input = document.querySelector<HTMLInputElement>('#create-directory-name');
    expect(document.activeElement).toBe(input);
    if (!input) throw new Error('input missing');
    input.value = 'New folder';
    input.dispatchEvent(new InputEvent('input', { bubbles: true }));
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    expect(confirm).toHaveBeenCalledWith('New folder');

    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    expect(cancel).toHaveBeenCalledOnce();
  });

  it('redraws its visible and accessible copy after a runtime locale switch', () => {
    m.mount(root, {
      view: () => m(CreateDirectoryDialog, { open: true, onConfirm: vi.fn(), onCancel: vi.fn() }),
    });
    m.redraw.sync();
    expect(root.textContent).toContain('New folder');

    setLocale('nl');
    m.redraw.sync();
    expect(root.textContent).toContain('Nieuwe map');
    expect(root.textContent).toContain('Annuleren');
  });
});
