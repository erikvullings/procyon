import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { AvailableAction } from './availability';
import { ContextMenu, clampContextMenuPosition } from './context-menu';

let root: HTMLElement;

const actions: readonly AvailableAction[] = [
  {
    action: {
      id: 'core.refresh',
      title: 'Refresh',
      category: 'navigation',
      defaultShortcuts: [],
      contextRequirements: {},
      source: { kind: 'core' },
    },
    available: true,
  },
  {
    action: {
      id: 'core.paste',
      title: 'Paste',
      category: 'fileOperations',
      defaultShortcuts: [],
      contextRequirements: {},
      source: { kind: 'core' },
    },
    available: false,
    reason: 'This location is read-only',
  },
];

beforeEach(() => {
  root = document.createElement('div');
  document.body.appendChild(root);
});

afterEach(() => {
  m.mount(root, null);
  root.remove();
});

describe('ContextMenu', () => {
  it('keeps the menu inside the viewport near the right and bottom edges', () => {
    expect(clampContextMenuPosition(790, 590, 180, 120, 800, 600)).toEqual({
      x: 612,
      y: 472,
    });
  });

  it('keeps a large menu inside the margin on the opposite edges', () => {
    expect(clampContextMenuPosition(-20, -10, 180, 120, 800, 600)).toEqual({
      x: 8,
      y: 8,
    });
  });

  it('uses the usual viewport edge placement when no native child exists', () => {
    m.mount(root, {
      view: () =>
        m(ContextMenu, {
          open: true,
          x: window.innerWidth - 10,
          y: window.innerHeight - 10,
          actions,
          onClose: vi.fn(),
          onInvoke: vi.fn(),
        }),
    });
    const menu = root.querySelector<HTMLElement>('.fm-context-menu');
    if (menu === null) throw new Error('menu missing');
    menu.getBoundingClientRect = () => ({ width: 180, height: 120 }) as DOMRect;
    m.redraw.sync();
    expect(menu.style.left).toBe(`${window.innerWidth - 188}px`);
    expect(menu.style.top).toBe(`${window.innerHeight - 128}px`);
  });

  it('invokes available actions with Enter, disables unavailable ones, and returns focus', () => {
    const trigger = document.createElement('button');
    document.body.appendChild(trigger);
    trigger.focus();
    const onClose = vi.fn();
    const onInvoke = vi.fn();
    m.mount(root, {
      view: () => m(ContextMenu, { open: true, x: 10, y: 20, actions, onClose, onInvoke }),
    });

    const menu = root.querySelector<HTMLElement>('[role="menu"]');
    expect(menu).toBe(document.activeElement);
    expect(
      root.querySelector<HTMLButtonElement>('[title="This location is read-only"]')?.disabled,
    ).toBe(true);
    menu?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    expect(onInvoke).toHaveBeenCalledWith('core.refresh');
    expect(onClose).toHaveBeenCalledOnce();
    expect(document.activeElement).toBe(trigger);
    trigger.remove();
  });

  it('opens a capability-gated platform submenu from the existing context menu', () => {
    const onOpenPlatformSubmenu = vi.fn();
    m.mount(root, {
      view: () =>
        m(ContextMenu, {
          open: true,
          x: 10,
          y: 20,
          actions,
          platformSubmenu: {
            title: 'Services',
            onOpen: onOpenPlatformSubmenu,
          },
          onClose: vi.fn(),
          onInvoke: vi.fn(),
        }),
    });

    const platformItem = [
      ...root.querySelectorAll<HTMLButtonElement>('.fm-context-menu-item'),
    ].find((item) => item.textContent?.includes('Services'));
    platformItem?.click();

    expect(onOpenPlatformSubmenu).toHaveBeenCalledOnce();
  });

  it('shows recommended applications beside Open With and invokes the selected bundle', async () => {
    const onChoose = vi.fn();
    const onInvoke = vi.fn();
    const onClose = vi.fn();
    const openWith: AvailableAction = {
      action: {
        id: 'core.openWith',
        title: 'Open With',
        category: 'navigation',
        defaultShortcuts: [],
        contextRequirements: {},
        source: { kind: 'core' },
      },
      available: true,
    };
    const load = vi
      .fn()
      .mockResolvedValue([{ name: 'Preview', path: '/Applications/Preview.app' }]);
    const iconFor = vi.fn().mockResolvedValue(new Uint8Array([137, 80, 78, 71]));
    m.mount(root, {
      view: () =>
        m(ContextMenu, {
          open: true,
          x: 10,
          y: 20,
          actions: [openWith],
          openWithSubmenu: { load, iconFor, onChoose, onOther: vi.fn() },
          onClose,
          onInvoke,
        }),
    });

    root.querySelector<HTMLButtonElement>('[aria-haspopup="menu"]')?.click();
    await vi.waitFor(() => expect(root.querySelectorAll('[role="menu"]')).toHaveLength(2));
    await vi.waitFor(() => expect(root.textContent).toContain('Preview'));
    expect(load).toHaveBeenCalledOnce();
    expect(onInvoke).not.toHaveBeenCalled();
    const item = [...root.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')].find((button) =>
      button.textContent?.includes('Preview'),
    );
    await vi.waitFor(() =>
      expect(item?.querySelector('img')?.getAttribute('src')).toMatch(/^data:image\/png;base64,/),
    );
    item?.click();
    expect(onChoose).toHaveBeenCalledWith('/Applications/Preview.app');
    expect(onClose).toHaveBeenCalledOnce();
  });

  it('moves keyboard focus into a submenu already opened by hover', async () => {
    const openWith: AvailableAction = {
      action: {
        id: 'core.openWith',
        title: 'Open With',
        category: 'navigation',
        defaultShortcuts: [],
        contextRequirements: {},
        source: { kind: 'core' },
      },
      available: true,
    };
    m.mount(root, {
      view: () =>
        m(ContextMenu, {
          open: true,
          x: 10,
          y: 20,
          actions: [openWith],
          openWithSubmenu: {
            load: async () => [{ name: 'Preview', path: '/Applications/Preview.app' }],
            iconFor: async () => undefined,
            onChoose: vi.fn(),
            onOther: vi.fn(),
          },
          onClose: vi.fn(),
          onInvoke: vi.fn(),
        }),
    });
    root
      .querySelector<HTMLElement>('[aria-haspopup="menu"]')
      ?.dispatchEvent(new MouseEvent('mouseenter', { bubbles: true }));
    await vi.waitFor(() => expect(root.textContent).toContain('Preview'));
    expect(root.querySelector('[aria-haspopup="menu"]')?.getAttribute('aria-expanded')).toBe(
      'true',
    );
    root
      .querySelector<HTMLElement>('[role="menu"]')
      ?.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true }));
    expect(document.activeElement?.textContent).toContain('Preview');
  });

  it('reports both measured menu surfaces and clears them on keyboard dismissal', async () => {
    const onBoundsChange = vi.fn();
    let open = true;
    const openWith: AvailableAction = {
      action: {
        id: 'core.openWith',
        title: 'Open With',
        category: 'navigation',
        defaultShortcuts: [],
        contextRequirements: {},
        source: { kind: 'core' },
      },
      available: true,
    };
    m.mount(root, {
      view: () =>
        m(ContextMenu, {
          open,
          x: 530,
          y: 150,
          actions: [openWith],
          openWithSubmenu: {
            load: async () => [{ name: 'Preview', path: '/Applications/Preview.app' }],
            iconFor: async () => undefined,
            onChoose: vi.fn(),
            onOther: vi.fn(),
          },
          onBoundsChange,
          onClose: () => {
            open = false;
          },
          onInvoke: vi.fn(),
        }),
    });
    const menu = root.querySelector<HTMLElement>(
      '.fm-context-menu:not(.fm-context-menu-open-with)',
    );
    if (menu === null) throw new Error('menu missing');
    const mainRect = { left: 530, top: 150, right: 620, bottom: 300, width: 90, height: 150 };
    menu.getBoundingClientRect = () => mainRect as DOMRect;
    window.dispatchEvent(new Event('resize'));
    expect(onBoundsChange).toHaveBeenLastCalledWith([mainRect]);
    root.querySelector<HTMLButtonElement>('[aria-haspopup="menu"]')?.click();
    await vi.waitFor(() => expect(root.querySelectorAll('[role="menu"]')).toHaveLength(2));
    const submenu = root.querySelector<HTMLElement>('.fm-context-menu-open-with');
    if (submenu === null) throw new Error('submenu missing');
    const submenuRect = { left: 620, top: 150, right: 820, bottom: 300, width: 200, height: 150 };
    submenu.getBoundingClientRect = () => submenuRect as DOMRect;
    m.redraw.sync();
    expect(onBoundsChange).toHaveBeenLastCalledWith([mainRect, submenuRect]);
    menu.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    m.redraw.sync();
    expect(onBoundsChange).toHaveBeenLastCalledWith([]);
  });
});
