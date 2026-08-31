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
});
