import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { MockFileManagerClient } from '../api/client/mock-file-manager-client';
import type { EntrySummary } from '../models';
import { ViewerWindow } from './viewer-window';

let root: HTMLElement;

beforeEach(() => {
  root = document.createElement('div');
  document.body.append(root);
});

afterEach(() => {
  m.mount(root, null);
  root.remove();
  vi.restoreAllMocks();
});

const entry: EntrySummary = {
  id: 'entry-env',
  location: { providerId: 'local', uri: 'file:///home/demo/.env' },
  name: '.env',
  kind: 'file',
  hidden: true,
  readOnly: false,
  extension: 'env',
  metadataRevision: 1,
};

describe('ViewerWindow', () => {
  it('mounts only the viewer for the requested file and applies the saved appearance', async () => {
    const client = new MockFileManagerClient();
    vi.spyOn(client, 'getSettings').mockResolvedValue({
      ...(await client.getSettings()),
      fontSize: 17,
    });
    m.mount(root, { view: () => m(ViewerWindow, { client, request: { entry } }) });

    await vi.waitFor(() => expect(root.querySelector('.fm-file-viewer')).not.toBeNull());
    expect(document.title).toBe('.env');
    expect(root.querySelector('.fm-file-viewer')?.textContent).toContain('.env');
    expect(root.querySelector('.fm-pane')).toBeNull();
    expect(document.documentElement.style.getPropertyValue('--fm-font-size')).toBe('17px');
  });

  it('closes the window on Escape but not while typing in a field', async () => {
    const close = vi.spyOn(globalThis, 'close').mockImplementation(() => undefined);
    const client = new MockFileManagerClient();
    m.mount(root, { view: () => m(ViewerWindow, { client, request: { entry } }) });
    await vi.waitFor(() => expect(root.querySelector('.fm-file-viewer')).not.toBeNull());

    const input = document.createElement('input');
    root.append(input);
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    expect(close).not.toHaveBeenCalled();

    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    await vi.waitFor(() => expect(close).toHaveBeenCalledTimes(1));
  });

  it('stops listening for shortcuts once unmounted', async () => {
    const close = vi.spyOn(globalThis, 'close').mockImplementation(() => undefined);
    const client = new MockFileManagerClient();
    m.mount(root, { view: () => m(ViewerWindow, { client, request: { entry } }) });
    await vi.waitFor(() => expect(root.querySelector('.fm-file-viewer')).not.toBeNull());
    m.mount(root, null);
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    await Promise.resolve();
    expect(close).not.toHaveBeenCalled();
  });
});
