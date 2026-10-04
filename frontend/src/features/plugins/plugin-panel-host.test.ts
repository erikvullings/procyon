import m from 'mithril';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { MockFileManagerClient } from '../../api/client/mock-file-manager-client';
import { ContextMenu } from '../commands/context-menu';
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

  it('keeps overlapping menu items clickable, including a submenu, then restores the native panel', async () => {
    const client = new MockFileManagerClient();
    vi.spyOn(client, 'openPluginPanel').mockResolvedValue('plugin-spa-test');
    vi.spyOn(client, 'updatePluginPanelBounds').mockResolvedValue();
    let nativeVisible = true;
    const setVisible = vi
      .spyOn(client, 'setPluginPanelVisible')
      .mockImplementation(async (...args: unknown[]) => {
        nativeVisible = args[1] === true;
      });
    const close = vi.spyOn(client, 'closePluginPanel').mockResolvedValue();
    const onChoose = vi.fn();
    let menuOpen = false;
    let overlayBounds: readonly DOMRect[] = [];
    document.body.appendChild(root);
    m.mount(root, {
      view: () => [
        m(PluginPanelHost, {
          panelId: 1,
          tabId: 'tab-1',
          client,
          pluginId: 'example.svgo',
          actionId: 'example.svgo.open',
          location: { providerId: 'local', uri: 'file:///drawing.svg' },
          title: 'SVGO',
          active: true,
          overlayBounds,
          onError: (error) => {
            throw error;
          },
          onCloseRequest: vi.fn(),
        }),
        m(ContextMenu, {
          open: menuOpen,
          x: 530,
          y: 150,
          actions: [
            {
              action: {
                id: 'core.openWith',
                title: 'Open With',
                category: 'navigation',
                defaultShortcuts: [],
                contextRequirements: {},
                source: { kind: 'core' },
              },
              available: true,
            },
          ],
          openWithSubmenu: {
            load: async () => [{ name: 'Preview', path: '/Applications/Preview.app' }],
            iconFor: async () => undefined,
            onChoose,
            onOther: vi.fn(),
          },
          onBoundsChange: (bounds) => {
            overlayBounds = bounds;
            m.redraw();
          },
          onClose: () => {
            menuOpen = false;
            overlayBounds = [];
          },
          onInvoke: vi.fn(),
        }),
      ],
    });
    const surface = root.querySelector<HTMLElement>('.fm-plugin-panel-surface');
    if (surface === null) throw new Error('panel surface missing');
    surface.getBoundingClientRect = () =>
      ({ left: 640, right: 1280, top: 80, bottom: 480, width: 640, height: 400 }) as DOMRect;
    window.dispatchEvent(new Event('resize'));
    await vi.waitFor(() => expect(client.openPluginPanel).toHaveBeenCalledOnce());
    menuOpen = true;
    m.redraw.sync();
    const menu = root.querySelector<HTMLElement>(
      '.fm-context-menu:not(.fm-context-menu-open-with)',
    );
    if (menu === null) throw new Error('context menu missing');
    menu.getBoundingClientRect = () =>
      ({ left: 530, right: 720, top: 150, bottom: 300, width: 190, height: 150 }) as DOMRect;
    m.redraw.sync();
    await vi.waitFor(() => expect(nativeVisible).toBe(false));
    const openWith = root.querySelector<HTMLButtonElement>('[aria-haspopup="menu"]');
    if (openWith === null) throw new Error('Open With missing');
    openWith.click();
    await vi.waitFor(() => expect(root.textContent).toContain('Preview'));
    const preview = [...root.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')].find(
      (button) => button.textContent?.includes('Preview'),
    );
    expect(preview).toBeDefined();
    expect(nativeVisible).toBe(false);
    // A native child would receive the pointer at this overlapping coordinate while visible.
    const hitTest = (item: HTMLButtonElement | undefined, x: number) =>
      nativeVisible && x >= 640 ? undefined : item;
    expect(hitTest(preview, 680)).toBe(preview);
    hitTest(preview, 680)?.click();
    m.redraw.sync();
    await vi.waitFor(() => expect(nativeVisible).toBe(true));
    expect(onChoose).toHaveBeenCalledWith('/Applications/Preview.app');
    expect(setVisible).toHaveBeenCalledWith('plugin-spa-test', false);
    expect(close).not.toHaveBeenCalled();
  });

  it.each([
    { side: 'right', panelLeft: 640, menuLeft: 530, menuRight: 720 },
    { side: 'left', panelLeft: 0, menuLeft: 570, menuRight: 770 },
  ])(
    'hides only for actual overlay intersections with the plugin on the $side',
    async ({ panelLeft, menuLeft, menuRight }) => {
      const client = new MockFileManagerClient();
      vi.spyOn(client, 'openPluginPanel').mockResolvedValue('plugin-spa-test');
      vi.spyOn(client, 'updatePluginPanelBounds').mockResolvedValue();
      vi.spyOn(client, 'closePluginPanel').mockResolvedValue();
      const visible = vi.spyOn(client, 'setPluginPanelVisible').mockResolvedValue();
      let panelWidth = 640;
      let overlayBounds: readonly DOMRect[] = [];
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
            title: 'SVGO',
            active,
            overlayBounds,
            onError: (error) => {
              throw error;
            },
            onCloseRequest: vi.fn(),
          }),
      });
      const surface = root.querySelector<HTMLElement>('.fm-plugin-panel-surface');
      if (surface === null) throw new Error('panel surface missing');
      surface.getBoundingClientRect = () =>
        ({
          left: panelLeft,
          right: panelLeft + panelWidth,
          top: 80,
          bottom: 480,
          width: panelWidth,
          height: 400,
        }) as DOMRect;
      window.dispatchEvent(new Event('resize'));
      await vi.waitFor(() => expect(client.openPluginPanel).toHaveBeenCalledOnce());
      const bounds = (left: number, right: number, top = 150): DOMRect =>
        ({ left, right, top, bottom: top + 100 }) as DOMRect;
      overlayBounds = [bounds(menuLeft, menuRight, 500)];
      m.redraw.sync();
      expect(visible).not.toHaveBeenCalled();
      overlayBounds = [bounds(400, 600)];
      m.redraw.sync();
      if (panelLeft === 0) {
        await vi.waitFor(() => expect(visible).toHaveBeenCalledWith('plugin-spa-test', false));
        overlayBounds = [bounds(680, 780)];
        m.redraw.sync();
        await vi.waitFor(() => expect(visible).toHaveBeenCalledWith('plugin-spa-test', true));
      } else {
        expect(visible).not.toHaveBeenCalled();
      }
      overlayBounds = [bounds(menuLeft, menuRight)];
      m.redraw.sync();
      await vi.waitFor(() => expect(visible).toHaveBeenLastCalledWith('plugin-spa-test', false));
      overlayBounds = [bounds(300, 400), bounds(menuLeft, menuRight)];
      m.redraw.sync();
      expect(visible).toHaveBeenLastCalledWith('plugin-spa-test', false);
      overlayBounds = [bounds(300, 400)];
      m.redraw.sync();
      await vi.waitFor(() =>
        expect(visible).toHaveBeenLastCalledWith('plugin-spa-test', panelLeft !== 0),
      );
      if (panelLeft === 0) {
        overlayBounds = [bounds(700, 800)];
        m.redraw.sync();
        await vi.waitFor(() => expect(visible).toHaveBeenLastCalledWith('plugin-spa-test', true));
      }
      panelWidth = 60;
      overlayBounds = [bounds(panelLeft + 50, panelLeft + 250)];
      m.redraw.sync();
      window.dispatchEvent(new Event('resize'));
      await vi.waitFor(() => expect(visible).toHaveBeenLastCalledWith('plugin-spa-test', false));
      active = false;
      overlayBounds = [];
      m.redraw.sync();
      expect(visible).toHaveBeenLastCalledWith('plugin-spa-test', false);
      active = true;
      m.redraw.sync();
      await vi.waitFor(() => expect(visible).toHaveBeenLastCalledWith('plugin-spa-test', true));
    },
  );
});
