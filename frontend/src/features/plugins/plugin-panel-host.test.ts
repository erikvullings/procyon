import m from 'mithril';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { MockFileManagerClient } from '../../api/client/mock-file-manager-client';
import { PluginPanelHost } from './plugin-panel-host';

describe('PluginPanelHost', () => {
  const root = document.createElement('div');
  afterEach(() => {
    m.mount(root, null);
    root.remove();
  });

  it('places, resizes and closes the isolated child WebView with the pane surface', async () => {
    const client = new MockFileManagerClient();
    const open = vi.spyOn(client, 'openPluginPanel').mockResolvedValue('plugin-spa-test');
    const update = vi.spyOn(client, 'updatePluginPanelBounds').mockResolvedValue();
    const close = vi.spyOn(client, 'closePluginPanel').mockResolvedValue();
    const onClose = vi.fn(() => m.mount(root, null));
    const onError = vi.fn();
    document.body.appendChild(root);
    m.mount(root, {
      view: () =>
        m(PluginPanelHost, {
          panelId: 1,
          tabId: 'tab-1',
          client,
          pluginId: 'example.svgo',
          actionId: 'example.svgo.open',
          location: { providerId: 'local', uri: 'file:///drawing.svg' },
          title: 'SVGO — drawing.svg',
          onClose,
          onError,
        }),
    });
    const surface = root.querySelector<HTMLElement>('.fm-plugin-panel-surface');
    if (surface === null) throw new Error('panel surface missing');
    let width = 320;
    surface.getBoundingClientRect = () => ({ left: 260, top: 80, width, height: 400 }) as DOMRect;
    window.dispatchEvent(new Event('resize'));
    await vi.waitFor(() =>
      expect(open).toHaveBeenCalledWith(
        'example.svgo',
        'example.svgo.open',
        { providerId: 'local', uri: 'file:///drawing.svg' },
        { x: 260, y: 80, width: 320, height: 400 },
      ),
    );
    width = 280;
    window.dispatchEvent(new Event('resize'));
    await vi.waitFor(() =>
      expect(update).toHaveBeenCalledWith('plugin-spa-test', {
        x: 260,
        y: 80,
        width: 280,
        height: 400,
      }),
    );
    root.querySelector<HTMLButtonElement>('.fm-plugin-panel-close')?.click();
    await vi.waitFor(() => expect(close).toHaveBeenCalledWith('plugin-spa-test'));
    expect(onClose).toHaveBeenCalledOnce();
    expect(onError).not.toHaveBeenCalled();
  });
});
