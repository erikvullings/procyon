import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Connection, Location } from '../../models';
import { OperationConfirmationDialog } from './operation-confirmation-dialog';

const source: Location = {
  providerId: 'local',
  uri: 'file:///source%20folder%23one/source%20file.txt',
};
const destination: Location = { providerId: 'local', uri: 'file:///target%20folder' };
const remoteDestination: Location = {
  providerId: 'sftp',
  uri: 'sftp://connection-id/target%20folder',
};
const connection: Connection = {
  id: 'connection-id',
  name: 'Home Assistant',
  kind: 'ssh',
  configuration: {
    kind: 'ssh',
    host: 'homeassistant.local',
    port: 22,
    username: 'root',
    authentication: 'agent',
    hostKeyPolicy: 'promptOnFirstUse',
    startPath: '/mnt/kingston',
  },
  status: 'connected',
  hasCredential: false,
  createdAt: '2026-09-20T10:00:00Z',
  updatedAt: '2026-09-20T10:00:00Z',
};
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
          connections: [],
          onConfirm,
          onCancel: vi.fn(),
        }),
    });
    m.redraw.sync();

    const dialog = document.querySelector('[role="alertdialog"]');
    expect(dialog?.querySelector('h4')?.textContent).toBe('Copy 1 item?');
    expect(dialog?.querySelector('.fm-operation-confirmation-summary')).toBeNull();
    const facts = document.querySelector('.fm-operation-confirmation-facts');
    expect(facts?.textContent).toContain('Source');
    expect(facts?.textContent).toContain('/source folder#one');
    expect(facts?.textContent).toContain('Destination');
    expect(facts?.textContent).toContain('/target folder');
    expect(facts?.textContent).not.toContain('%20');
    expect(facts?.textContent).not.toContain('file://');
    expect(
      facts?.querySelector('.fm-operation-confirmation-destination .fm-operation-path-leaf')
        ?.textContent,
    ).toBe('target folder');
    expect(document.activeElement?.textContent?.trim()).toBe('Copy');
    document.activeElement?.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'Tab', bubbles: true, cancelable: true }),
    );
    expect(document.activeElement?.textContent?.trim()).toBe('Cancel');
    document.activeElement?.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'Tab', bubbles: true, cancelable: true }),
    );
    expect(document.activeElement?.textContent?.trim()).toBe('Copy');
    document.activeElement?.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }),
    );
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
            destination: remoteDestination,
          },
          connections: [connection],
          onConfirm: vi.fn(),
          onCancel: vi.fn(),
        }),
    });
    m.redraw.sync();

    const dialog = document.querySelector('[role="alertdialog"]');
    expect(dialog?.querySelector('h4')?.textContent).toBe('Move 3 items?');
    expect(dialog?.querySelector('.fm-operation-confirmation-summary')).toBeNull();
    const sourceLocations = [
      ...document.querySelectorAll('.fm-operation-confirmation-source .fm-operation-path'),
    ].map((element) => element.getAttribute('title'));
    expect(sourceLocations).toEqual(['/source folder#one', 'Home Assistant · /incoming']);
    expect(
      dialog
        ?.querySelector('.fm-operation-confirmation-destination .fm-operation-path')
        ?.getAttribute('title'),
    ).toBe('Home Assistant · /target folder');
    expect(dialog?.textContent).not.toContain('connection-id');
  });

  it('abbreviates the home directory in local paths', () => {
    m.mount(root, {
      view: () =>
        m(OperationConfirmationDialog, {
          request: {
            kind: 'copy',
            sources: [{ providerId: 'local', uri: 'file:///Users/ada/Downloads/v2612' }],
            destination: { providerId: 'local', uri: 'file:///Users/ada/OneDrive/Plaatjes' },
          },
          connections: [],
          homeDirectory: '/Users/ada/',
          onConfirm: vi.fn(),
          onCancel: vi.fn(),
        }),
    });
    m.redraw.sync();

    const titles = [...document.querySelectorAll('.fm-operation-path')].map((element) =>
      element.getAttribute('title'),
    );
    expect(titles).toEqual(['~/Downloads', '~/OneDrive/Plaatjes']);
  });

  it('marks Trash as destructive and can be cancelled', () => {
    const onCancel = vi.fn();
    m.mount(root, {
      view: () =>
        m(OperationConfirmationDialog, {
          request: { kind: 'trash', sources: [source] },
          connections: [],
          onConfirm: vi.fn(),
          onCancel,
        }),
    });
    m.redraw.sync();

    const dialog = document.querySelector('[role="alertdialog"]');
    expect(dialog?.querySelector('h4')?.textContent).toBe('Move 1 item to Trash?');
    const scope = dialog?.querySelector('.fm-operation-confirmation-focus-scope');
    expect(scope?.querySelector('.fm-operation-confirmation-destination')).toBeNull();
    expect(
      [...(scope?.querySelectorAll('.fm-affected-items-list li') ?? [])].map(
        (li) => li.textContent,
      ),
    ).toEqual(['source file.txt']);
    expect(document.activeElement?.textContent?.trim()).toBe('Move to Trash');
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
