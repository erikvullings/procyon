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

  it('keeps the main menu and its submenu in the source pane without obscuring the native panel', async () => {
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
          x: 610,
          y: 150,
          actions: [openWith],
          placementBounds: () =>
            ({ left: 0, top: 0, right: 640, bottom: 500, width: 640, height: 500 }) as DOMRect,
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
    const menu = root.querySelector<HTMLElement>('.fm-context-menu');
    if (menu === null) throw new Error('menu missing');
    menu.getBoundingClientRect = () =>
      ({
        left: Number.parseFloat(menu.style.left),
        right: Number.parseFloat(menu.style.left) + 190,
        top: 150,
        bottom: 300,
        width: 190,
        height: 150,
      }) as DOMRect;
    m.redraw.sync();
    expect(menu.style.left).toBe('442px');
    const trigger = root.querySelector<HTMLButtonElement>('[aria-haspopup="menu"]');
    if (trigger === null) throw new Error('Open With missing');
    trigger.getBoundingClientRect = () =>
      ({ left: 442, right: 632, top: 180, bottom: 210 }) as DOMRect;
    trigger.click();
    await vi.waitFor(() => expect(root.textContent).toContain('Preview'));
    const submenu = root.querySelector<HTMLElement>('.fm-context-menu-open-with');
    if (submenu === null) throw new Error('submenu missing');
    submenu.getBoundingClientRect = () => ({ width: 210, height: 110 }) as DOMRect;
    m.redraw.sync();
    expect(submenu.style.left).toBe('232px');
    expect(menu.style.maxWidth).toBe('624px');
    expect(submenu.style.maxWidth).toBe('624px');
  });

  it('constrains placement on the right and scrolls within a short source pane', () => {
    m.mount(root, {
      view: () =>
        m(ContextMenu, {
          open: true,
          x: 990,
          y: 390,
          actions,
          placementBounds: () =>
            ({ left: 500, right: 1000, top: 150, bottom: 400, width: 500, height: 250 }) as DOMRect,
          onClose: vi.fn(),
          onInvoke: vi.fn(),
        }),
    });
    const menu = root.querySelector<HTMLElement>('.fm-context-menu');
    if (menu === null) throw new Error('menu missing');
    menu.getBoundingClientRect = () => ({ width: 190, height: 150 }) as DOMRect;
    window.dispatchEvent(new Event('resize'));
    expect(menu.style.left).toBe('802px');
    expect(menu.style.top).toBe('242px');
    expect(menu.style.maxHeight).toBe('234px');
    expect(menu.style.overflowY).toBe('auto');
  });

  it('lets another row receive a right-click while suppressing the native fallback menu', () => {
    let open = true;
    const onClose = vi.fn(() => {
      open = false;
    });
    m.mount(root, {
      view: () => m(ContextMenu, { open, x: 10, y: 20, actions, onClose, onInvoke: vi.fn() }),
    });
    const backdrop = root.querySelector<HTMLElement>('.fm-context-menu-backdrop');
    const menu = root.querySelector<HTMLElement>('.fm-context-menu');
    expect(backdrop?.style.pointerEvents).toBe('none');
    expect(menu?.style.pointerEvents).toBe('auto');
    const secondRow = document.createElement('div');
    root.appendChild(secondRow);
    const onSecondMenu = vi.fn();
    secondRow.addEventListener('contextmenu', onSecondMenu);
    const event = new MouseEvent('contextmenu', { bubbles: true, cancelable: true, button: 2 });
    secondRow.dispatchEvent(event);
    expect(onSecondMenu).toHaveBeenCalledOnce();
    expect(event.defaultPrevented).toBe(true);
    expect(onClose).not.toHaveBeenCalled();
    secondRow.click();
    expect(onClose).toHaveBeenCalledOnce();
    m.redraw.sync();
    const afterClose = new MouseEvent('contextmenu', { bubbles: true, cancelable: true });
    secondRow.dispatchEvent(afterClose);
    expect(afterClose.defaultPrevented).toBe(false);
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

  it('groups new folder and the templates into one New submenu in the empty-area menu (task 0312)', async () => {
    const createDirectory: AvailableAction = {
      action: {
        id: 'core.createDirectory',
        title: 'New folder',
        category: 'fileOperations',
        defaultShortcuts: [],
        contextRequirements: {},
        source: { kind: 'core' },
      },
      available: true,
    };
    const newFromTemplate: AvailableAction = {
      action: {
        id: 'core.newFromTemplate',
        title: 'New File from Template…',
        category: 'fileOperations',
        defaultShortcuts: [],
        contextRequirements: {},
        source: { kind: 'core' },
      },
      available: true,
    };
    const load = vi.fn().mockResolvedValue([
      { id: 'to-do', label: 'To-do list' },
      { id: 'folder:meeting.md', label: 'Meeting note', detail: 'Folder: Notes' },
    ]);
    const onChoose = vi.fn();
    const onInvoke = vi.fn();
    const onClose = vi.fn();
    m.mount(root, {
      view: () =>
        m(ContextMenu, {
          open: true,
          x: 10,
          y: 20,
          actions: [createDirectory, newFromTemplate],
          newSubmenu: { title: 'New', load, onChoose },
          onClose,
          onInvoke,
        }),
    });

    // Only the submenu shows at the top level; its two actions live in the flyout.
    const top = [...root.querySelectorAll<HTMLButtonElement>('.fm-context-menu [role="menuitem"]')];
    expect(top.map((button) => button.textContent)).toEqual(['New']);

    top[0]?.click();
    await vi.waitFor(() => expect(root.textContent).toContain('Meeting note'));
    expect(load).toHaveBeenCalledOnce();
    expect(root.textContent).toContain('Folder: Notes');
    // The folder action is first in the flyout, then the templates.
    const flyout = root.querySelector('#fm-new-submenu');
    expect(
      [...(flyout?.querySelectorAll<HTMLButtonElement>('[role="menuitem"]') ?? [])].map(
        (button) => button.textContent,
      ),
    ).toEqual(['New folder', 'To-do list', 'Meeting noteFolder: Notes']);

    [...(flyout?.querySelectorAll<HTMLButtonElement>('[role="menuitem"]') ?? [])]
      .find((button) => button.textContent?.startsWith('Meeting note'))
      ?.click();
    expect(onChoose).toHaveBeenCalledWith('folder:meeting.md');
    expect(onClose).toHaveBeenCalledOnce();
    expect(onInvoke).not.toHaveBeenCalled();
  });

  it('offers the New submenu beside a selection, after the open group (task 0312)', () => {
    const open: AvailableAction = {
      action: {
        id: 'core.open',
        title: 'Open',
        category: 'navigation',
        defaultShortcuts: [],
        contextRequirements: {},
        source: { kind: 'core' },
      },
      available: true,
    };
    const createDirectory: AvailableAction = {
      action: {
        id: 'core.createDirectory',
        title: 'New folder',
        category: 'fileOperations',
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
          actions: [open, createDirectory],
          newSubmenu: { title: 'New', load: vi.fn().mockResolvedValue([]), onChoose: vi.fn() },
          onClose: vi.fn(),
          onInvoke: vi.fn(),
        }),
    });
    const top = [...root.querySelectorAll<HTMLButtonElement>('.fm-context-menu [role="menuitem"]')];
    expect(top.map((button) => button.textContent)).toEqual(['Open', 'New']);
  });

  it('opens the New submenu from the keyboard and offers the folder action first', async () => {
    const createDirectory: AvailableAction = {
      action: {
        id: 'core.createDirectory',
        title: 'New folder',
        category: 'fileOperations',
        defaultShortcuts: [],
        contextRequirements: {},
        source: { kind: 'core' },
      },
      available: true,
    };
    const onInvoke = vi.fn();
    m.mount(root, {
      view: () =>
        m(ContextMenu, {
          open: true,
          x: 10,
          y: 20,
          actions: [createDirectory],
          newSubmenu: {
            title: 'New',
            load: vi.fn().mockResolvedValue([]),
            onChoose: vi.fn(),
          },
          onClose: vi.fn(),
          onInvoke,
        }),
    });

    const menu = root.querySelector<HTMLElement>('.fm-context-menu');
    menu?.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true }));
    await vi.waitFor(() => expect(root.querySelector('#fm-new-submenu')).not.toBeNull());
    root.querySelector<HTMLButtonElement>('#fm-new-submenu [role="menuitem"]')?.click();
    expect(onInvoke).toHaveBeenCalledWith('core.createDirectory');
  });

  it('keeps everyday actions at the top level, groups type-specific ones, and leaves out the hidden', async () => {
    const act = (id: string, title: string, available = true, hidden = false): AvailableAction => ({
      action: {
        id,
        title,
        category: 'fileOperations',
        defaultShortcuts: [],
        contextRequirements: {},
        source: { kind: 'core' },
      },
      available,
      ...(hidden ? { hidden: true, reason: 'not for this selection' } : {}),
    });
    const onInvoke = vi.fn();
    const onClose = vi.fn();
    m.mount(root, {
      view: () =>
        m(ContextMenu, {
          open: true,
          x: 10,
          y: 20,
          actions: [
            act('core.copyPath', 'Copy Full Path'),
            act('client.play', 'Play'),
            act('client.addToQueue', 'Add to queue', false),
            act('core.pack', 'Pack to Archive'),
            act('core.extract', 'Extract Archive', false),
            act('core.compare', 'Compare files', true, true),
          ],
          onClose,
          onInvoke,
        }),
    });

    const top = [...root.querySelectorAll<HTMLButtonElement>('.fm-context-menu [role="menuitem"]')];
    expect(top.map((button) => button.textContent)).toEqual(['Copy Full Path', 'Media', 'Archive']);
    // The hidden entry is not rendered at all, not even greyed out.
    expect(root.textContent).not.toContain('Compare files');
    // The group rows disable only when nothing in them is available.
    expect(top[2]?.disabled).toBe(false);

    top[1]?.click();
    await vi.waitFor(() => expect(root.querySelector('#fm-group-submenu')).not.toBeNull());
    const flyout = root.querySelector('#fm-group-submenu');
    expect(
      [...(flyout?.querySelectorAll<HTMLButtonElement>('[role="menuitem"]') ?? [])].map(
        (button) => [button.textContent, button.disabled],
      ),
    ).toEqual([
      ['Play', false],
      ['Add to queue', true],
    ]);
    root.querySelector<HTMLButtonElement>('#fm-group-submenu [role="menuitem"]')?.click();
    expect(onInvoke).toHaveBeenCalledWith('client.play');
    expect(onClose).toHaveBeenCalledOnce();
  });

  it('closes an open Open With flyout when the pointer moves onto the New row (task 0312)', async () => {
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
    const createDirectory: AvailableAction = {
      action: {
        id: 'core.createDirectory',
        title: 'New folder',
        category: 'fileOperations',
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
          actions: [openWith, createDirectory],
          openWithSubmenu: {
            load: vi.fn().mockResolvedValue([]),
            iconFor: vi.fn(),
            onChoose: vi.fn(),
            onOther: vi.fn(),
          },
          newSubmenu: { title: 'New', load: vi.fn().mockResolvedValue([]), onChoose: vi.fn() },
          onClose: vi.fn(),
          onInvoke: vi.fn(),
        }),
    });
    const rows = [
      ...root.querySelectorAll<HTMLButtonElement>('.fm-context-menu [role="menuitem"]'),
    ];
    rows[0]?.dispatchEvent(new MouseEvent('mouseenter'));
    await vi.waitFor(() => expect(root.querySelector('#fm-open-with-submenu')).not.toBeNull());
    rows[1]?.dispatchEvent(new MouseEvent('mouseenter'));
    await vi.waitFor(() => expect(root.querySelector('#fm-open-with-submenu')).toBeNull());
    await vi.waitFor(() => expect(root.querySelector('#fm-new-submenu')).not.toBeNull());
    expect(root.querySelector('#fm-open-with-submenu')).toBeNull();
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
