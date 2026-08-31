import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { DeleteWorkspaceDialog } from './delete-workspace-dialog';

let root: HTMLElement;

beforeEach(() => {
  root = document.createElement('div');
  document.body.appendChild(root);
});

afterEach(() => {
  m.mount(root, null);
  root.remove();
});

describe('DeleteWorkspaceDialog', () => {
  it('confirms before deleting the named workspace', () => {
    const onConfirm = vi.fn();
    const onCancel = vi.fn();
    m.mount(root, {
      view: () =>
        m(DeleteWorkspaceDialog, { open: true, workspaceName: 'Project X', onConfirm, onCancel }),
    });
    m.redraw.sync();

    expect(root.textContent).toContain('Project X');
    [...root.querySelectorAll('button')].find((b) => b.textContent === 'Delete')?.click();
    expect(onConfirm).toHaveBeenCalledOnce();

    [...root.querySelectorAll('button')].find((b) => b.textContent === 'Cancel')?.click();
    expect(onCancel).toHaveBeenCalledOnce();
  });

  it('uses dedicated compact modal chrome with a complete action footer', () => {
    m.mount(root, {
      view: () =>
        m(DeleteWorkspaceDialog, {
          open: true,
          workspaceName: 'Project X',
          onConfirm: vi.fn(),
          onCancel: vi.fn(),
        }),
    });
    m.redraw.sync();

    const dialog = root.querySelector<HTMLElement>('[role="dialog"]');
    expect(dialog?.classList.contains('fm-dense-modal')).toBe(true);
    expect(dialog?.classList.contains('fm-delete-workspace-modal')).toBe(true);
    expect(dialog?.querySelector(':scope > button.mm-modal-close-button')).not.toBeNull();
    expect(
      [...(dialog?.querySelectorAll('.modal-footer button') ?? [])].map(
        (button) => button.textContent,
      ),
    ).toEqual(['Cancel', 'Delete']);
  });

  it('renders nothing interactive while closed', () => {
    m.mount(root, {
      view: () =>
        m(DeleteWorkspaceDialog, {
          open: false,
          workspaceName: 'Project X',
          onConfirm: vi.fn(),
          onCancel: vi.fn(),
        }),
    });
    m.redraw.sync();

    const dialog = root.querySelector('[role="dialog"]');
    expect(dialog?.getAttribute('aria-hidden')).toBe('true');
  });
});
