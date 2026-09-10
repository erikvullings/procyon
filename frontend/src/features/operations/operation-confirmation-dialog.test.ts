import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Location } from '../../models';
import { OperationConfirmationDialog } from './operation-confirmation-dialog';

const source: Location = { providerId: 'local', uri: 'file:///source.txt' };
const destination: Location = { providerId: 'local', uri: 'file:///target' };
let root: HTMLElement;

beforeEach(() => {
  root = document.createElement('div');
  document.body.appendChild(root);
});

afterEach(() => {
  m.mount(root, null);
  root.remove();
});

describe('OperationConfirmationDialog', () => {
  it('describes and confirms a copy before it starts', () => {
    const onConfirm = vi.fn();
    m.mount(root, {
      view: () =>
        m(OperationConfirmationDialog, {
          request: { kind: 'copy', sources: [source], destination },
          onConfirm,
          onCancel: vi.fn(),
        }),
    });
    m.redraw.sync();

    expect(document.querySelector('[role="alertdialog"]')?.textContent).toContain(
      'Copy 1 item(s) to file:///target?',
    );
    [...document.querySelectorAll<HTMLButtonElement>('button')]
      .find((button) => button.textContent?.trim() === 'Copy')
      ?.click();
    expect(onConfirm).toHaveBeenCalledOnce();
  });

  it('marks Trash as destructive and can be cancelled', () => {
    const onCancel = vi.fn();
    m.mount(root, {
      view: () =>
        m(OperationConfirmationDialog, {
          request: { kind: 'trash', sources: [source] },
          onConfirm: vi.fn(),
          onCancel,
        }),
    });
    m.redraw.sync();

    const trash = [...document.querySelectorAll<HTMLButtonElement>('button')].find(
      (button) => button.textContent?.trim() === 'Move to Trash',
    );
    expect(trash?.dataset.destructive).toBe('true');
    [...document.querySelectorAll<HTMLButtonElement>('button')]
      .find((button) => button.textContent?.trim() === 'Cancel')
      ?.click();
    expect(onCancel).toHaveBeenCalledOnce();
  });
});
