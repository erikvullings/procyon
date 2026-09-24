import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { MockFileManagerClient } from '../../api/client/mock-file-manager-client';
import { AppUpdateDialog } from './app-update-dialog';

describe('AppUpdateDialog', () => {
  let root: HTMLElement;

  beforeEach(() => {
    root = document.createElement('div');
    document.body.appendChild(root);
  });

  afterEach(() => {
    m.mount(root, null);
    root.remove();
  });

  it('requires confirmation before installing and restarting', async () => {
    const client = new MockFileManagerClient();
    const install = vi.spyOn(client, 'installAppUpdate').mockResolvedValue(undefined);
    m.mount(root, {
      view: () =>
        m(AppUpdateDialog, {
          client,
          update: { currentVersion: '0.1.5', version: '0.1.6', body: 'Update notes' },
          onLater: vi.fn(),
        }),
    });
    m.redraw.sync();

    expect(root.textContent).toContain('Update Procyon 0.1.5 to 0.1.6?');
    expect(root.textContent).toContain('Update notes');
    expect(install).not.toHaveBeenCalled();

    root.querySelector<HTMLButtonElement>('.fm-update-install')?.click();
    await vi.waitFor(() => expect(install).toHaveBeenCalledOnce());
  });
});
