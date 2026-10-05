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

  it('resizes and hides the child WebView across tab switches without closing it', async () => {
    const client = new MockFileManagerClient();
    const open = vi.spyOn(client, 'openPluginPanel').mockResolvedValue('plugin-spa-test');
    const update = vi.spyOn(client, 'updatePluginPanelBounds').mockResolvedValue();
    const setVisible = vi.spyOn(client, 'setPluginPanelVisible').mockResolvedValue();
    const setTheme = vi.spyOn(client, 'setPluginPanelTheme').mockResolvedValue();
    const close = vi.spyOn(client, 'closePluginPanel').mockResolvedValue();
    const onError = vi.fn();
    let active = true;
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
          title: 'SVGO: drawing.svg',
          active,
          onError,
          onCloseRequest: vi.fn(),
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
        expect.any(String),
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
    active = false;
    m.redraw.sync();
    await vi.waitFor(() => expect(setVisible).toHaveBeenCalledWith('plugin-spa-test', false));
    expect(close).not.toHaveBeenCalled();
    expect(root.querySelector('.fm-plugin-panel-host')?.getAttribute('data-visible')).toBe('false');
    active = true;
    m.redraw.sync();
    await vi.waitFor(() => expect(setVisible).toHaveBeenCalledWith('plugin-spa-test', true));
    expect(open).toHaveBeenCalledOnce();
    const initialTheme = open.mock.calls[0]?.[4];
    document.documentElement.dataset.theme = initialTheme === 'dark' ? 'light' : 'dark';
    await vi.waitFor(() =>
      expect(setTheme).toHaveBeenCalledWith(
        'plugin-spa-test',
        initialTheme === 'dark' ? 'light' : 'dark',
      ),
    );
    document.documentElement.removeAttribute('data-theme');
    m.mount(root, null);
    await vi.waitFor(() => expect(close).toHaveBeenCalledWith('plugin-spa-test'));
    expect(onError).not.toHaveBeenCalled();
  });
});
