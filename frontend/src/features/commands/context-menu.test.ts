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
});
