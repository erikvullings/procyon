import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Location } from '../../models';
import { OperationConfirmationDialog } from './operation-confirmation-dialog';

const source: Location = {
  providerId: 'local',
  uri: 'file:///source%20folder%23one/source%20file.txt',
};
const destination: Location = { providerId: 'local', uri: 'file:///target%20folder' };
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

    expect(document.querySelector('[role="alertdialog"]')?.textContent).toContain('Copy 1 item?');
    const route = document.querySelector('.fm-operation-confirmation-route');
    expect(route?.textContent).toContain('Source');
    expect(route?.textContent).toContain('file:///source folder#one');
    expect(route?.textContent).toContain('Destination');
    expect(route?.textContent).toContain('file:///target folder');
    expect(route?.textContent).not.toContain('%20');
    expect(document.activeElement?.textContent?.trim()).toBe('Copy');
    document.activeElement?.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'Tab', bubbles: true, cancelable: true }),
    );
    expect(document.activeElement?.textContent?.trim()).toBe('Cancel');
    [...document.querySelectorAll<HTMLButtonElement>('button')]
      .find((button) => button.textContent?.trim() === 'Copy')
      ?.click();
    expect(onConfirm).toHaveBeenCalledOnce();
  });

  it('uses localized plurals and lists each distinct source directory', () => {
    m.mount(root, {
      view: () =>
        m(OperationConfirmationDialog, {
          request: {
            kind: 'move',
            sources: [
              source,
              { providerId: 'sftp', uri: 'sftp://connection-id/incoming/remote.txt' },
              { providerId: 'local', uri: 'file:///source%20folder%23one/other.txt' },
            ],
            destination,
          },
          onConfirm: vi.fn(),
          onCancel: vi.fn(),
        }),
    });
    m.redraw.sync();

    expect(document.querySelector('[role="alertdialog"]')?.textContent).toContain('Move 3 items?');
    const sourceLocations = [
      ...document.querySelectorAll('.fm-operation-confirmation-source code'),
    ].map((element) => element.textContent);
    expect(sourceLocations).toEqual(['file:///source folder#one', 'sftp://connection-id/incoming']);
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

    expect(document.querySelector('[role="alertdialog"]')?.textContent).toContain(
      'Move 1 item to Trash?',
    );
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
