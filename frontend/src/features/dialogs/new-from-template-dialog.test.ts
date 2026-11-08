import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { DEFAULT_FILE_TEMPLATES } from '../templates/file-templates';
import { NewFromTemplateDialog } from './new-from-template-dialog';

let root: HTMLElement;

beforeEach(() => {
  root = document.createElement('div');
  document.body.append(root);
});

afterEach(() => {
  m.mount(root, null);
  root.remove();
});

function mount(open: boolean, onCreate = vi.fn(), onCancel = vi.fn()): void {
  m.mount(root, {
    view: () =>
      m(NewFromTemplateDialog, { open, templates: DEFAULT_FILE_TEMPLATES, onCreate, onCancel }),
  });
  m.redraw.sync();
}

describe('NewFromTemplateDialog', () => {
  it('pre-fills and selects the first template name, and switches it with the template', () => {
    mount(true);
    const input = root.querySelector<HTMLInputElement>('#new-from-template-name');
    expect(input?.value).toBe('New file.txt');
    expect(input?.selectionStart).toBe(0);
    expect(input?.selectionEnd).toBe('New file.txt'.length);

    const markdown = [...root.querySelectorAll<HTMLButtonElement>('[role=radio]')].find(
      (button) => button.textContent === 'Markdown note',
    );
    markdown?.click();
    m.redraw.sync();
    expect(input?.value).toBe('Note.md');
    expect(markdown?.getAttribute('aria-checked')).toBe('true');
  });

  it('creates the chosen template under the edited name on Enter', () => {
    const onCreate = vi.fn();
    mount(true, onCreate);
    const input = root.querySelector<HTMLInputElement>('#new-from-template-name');
    if (!input) throw new Error('name input missing');
    input.value = 'Meeting.txt';
    input.dispatchEvent(new InputEvent('input', { bubbles: true }));
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    expect(onCreate).toHaveBeenCalledWith(DEFAULT_FILE_TEMPLATES[0], 'Meeting.txt');
  });

  it('refuses an invalid name', () => {
    const onCreate = vi.fn();
    mount(true, onCreate);
    const input = root.querySelector<HTMLInputElement>('#new-from-template-name');
    if (!input) throw new Error('name input missing');
    input.value = '../evil.txt';
    input.dispatchEvent(new InputEvent('input', { bubbles: true }));
    m.redraw.sync();
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    expect(onCreate).not.toHaveBeenCalled();
    expect(root.querySelector('.fm-field-error')).not.toBeNull();
  });
});
