import m, { type FactoryComponent } from 'mithril';

import { t } from '../../i18n';
import type { AvailableAction } from './availability';

export interface ContextMenuAttrs {
  readonly open: boolean;
  readonly x: number;
  readonly y: number;
  readonly actions: readonly AvailableAction[];
  readonly platformSubmenu?: {
    readonly title: string;
    readonly onOpen: () => void;
  };
  readonly onClose: () => void;
  readonly onInvoke: (actionId: string) => void;
}

const CONTEXT_MENU_VIEWPORT_MARGIN = 8;

export function clampContextMenuPosition(
  x: number,
  y: number,
  width: number,
  height: number,
  viewportWidth: number,
  viewportHeight: number,
  margin = CONTEXT_MENU_VIEWPORT_MARGIN,
): { x: number; y: number } {
  const maxX = Math.max(margin, viewportWidth - width - margin);
  const maxY = Math.max(margin, viewportHeight - height - margin);
  return {
    x: Math.max(margin, Math.min(x, maxX)),
    y: Math.max(margin, Math.min(y, maxY)),
  };
}

/** Keyboard-navigable in-window menu styled with the app's Materialized theme tokens. */
export const ContextMenu: FactoryComponent<ContextMenuAttrs> = () => {
  let activeIndex = 0;
  let previousFocus: HTMLElement | undefined;

  function close(attrs: ContextMenuAttrs): void {
    attrs.onClose();
    previousFocus?.focus();
    previousFocus = undefined;
  }

  function invoke(attrs: ContextMenuAttrs, index: number): void {
    if (index === attrs.actions.length && attrs.platformSubmenu !== undefined) {
      attrs.platformSubmenu.onOpen();
      close(attrs);
      return;
    }
    const item = attrs.actions[index];
    if (item === undefined || !item.available) return;
    attrs.onInvoke(item.action.id);
    close(attrs);
  }

  return {
    onupdate: ({ attrs }) => {
      if (attrs.open && previousFocus === undefined)
        previousFocus = document.activeElement as HTMLElement;
    },
    view: ({ attrs }) => {
      if (!attrs.open) return undefined;
      const itemCount = attrs.actions.length + (attrs.platformSubmenu === undefined ? 0 : 1);
      activeIndex = Math.min(activeIndex, Math.max(0, itemCount - 1));
      const menuItems = attrs.actions.map((item, index) =>
        m(
          'button.fm-context-menu-item',
          {
            key: item.action.id,
            type: 'button',
            role: 'menuitem',
            disabled: !item.available,
            tabindex: index === activeIndex ? 0 : -1,
            title: item.reason,
            onclick: () => invoke(attrs, index),
          },
          item.action.title,
        ),
      );
      if (attrs.platformSubmenu !== undefined) {
        menuItems.push(
          m(
            'button.fm-context-menu-item.fm-context-menu-submenu',
            {
              key: 'platform-submenu',
              type: 'button',
              role: 'menuitem',
              'aria-haspopup': 'menu',
              tabindex: attrs.actions.length === activeIndex ? 0 : -1,
              onclick: () => invoke(attrs, attrs.actions.length),
            },
            attrs.platformSubmenu.title,
          ),
        );
      }
      return m('.fm-context-menu-backdrop', { onclick: () => close(attrs) }, [
        m(
          '.fm-context-menu',
          {
            role: 'menu',
            tabindex: -1,
            'aria-label': t('contextMenu', 'directoryActions'),
            style: { left: `${attrs.x}px`, top: `${attrs.y}px` },
            oncreate: ({ dom }) => {
              positionMenu(dom as HTMLElement, attrs);
              if (previousFocus === undefined)
                previousFocus = document.activeElement as HTMLElement;
              (dom as HTMLElement).focus();
            },
            onupdate: ({ dom }) => positionMenu(dom as HTMLElement, attrs),
            onclick: (event: MouseEvent) => event.stopPropagation(),
            onkeydown: (event: KeyboardEvent) => {
              if (event.key === 'Escape') {
                event.preventDefault();
                close(attrs);
              } else if (event.key === 'ArrowDown') {
                event.preventDefault();
                activeIndex = Math.min(activeIndex + 1, itemCount - 1);
                m.redraw();
              } else if (event.key === 'ArrowUp') {
                event.preventDefault();
                activeIndex = Math.max(activeIndex - 1, 0);
                m.redraw();
              } else if (event.key === 'Enter' || event.key === ' ') {
                event.preventDefault();
                invoke(attrs, activeIndex);
              }
            },
          },
          menuItems,
        ),
      ]);
    },
  };
};

function positionMenu(menu: HTMLElement, attrs: ContextMenuAttrs): void {
  const rect = menu.getBoundingClientRect();
  const position = clampContextMenuPosition(
    attrs.x,
    attrs.y,
    rect.width,
    rect.height,
    window.innerWidth,
    window.innerHeight,
  );
  menu.style.left = `${position.x}px`;
  menu.style.top = `${position.y}px`;
}
