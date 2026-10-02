import m, { type FactoryComponent } from 'mithril';

import type { OpenWithApplication } from '../../api/client/file-manager-client';
import { t } from '../../i18n';
import type { SelectionPlatform } from '../selection/keybindings';
import {
  type AvailableAction,
  contextMenuGroup,
  DESTRUCTIVE_CONTEXT_ACTION_IDS,
} from './availability';
import { formatShortcut } from './shortcut-label';

export interface ContextMenuAttrs {
  readonly open: boolean;
  readonly x: number;
  readonly y: number;
  readonly actions: readonly AvailableAction[];
  readonly platform?: SelectionPlatform;
  readonly platformSubmenu?: {
    readonly title: string;
    readonly onOpen: () => void;
  };
  readonly openWithSubmenu?: {
    readonly load: () => Promise<readonly OpenWithApplication[]>;
    readonly iconFor: (path: string) => Promise<Uint8Array | undefined>;
    readonly onChoose: (path: string) => void;
    readonly onOther: () => void;
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
  let submenu: 'closed' | 'loading' | 'ready' | 'error' = 'closed';
  let applications: readonly OpenWithApplication[] = [];
  const icons = new Map<string, string>();
  let loadGeneration = 0;
  let openWithItem: HTMLElement | undefined;
  let focusApplication = false;

  function iconDataUri(bytes: Uint8Array): string {
    let binary = '';
    for (const byte of bytes) binary += String.fromCharCode(byte);
    return `data:image/png;base64,${btoa(binary)}`;
  }

  function showOpenWith(attrs: ContextMenuAttrs, fromKeyboard = false): void {
    if (attrs.openWithSubmenu === undefined) return;
    if (submenu !== 'closed') {
      if (fromKeyboard) {
        if (submenu === 'loading') focusApplication = true;
        else
          openWithItem
            ?.closest('.fm-context-menu-backdrop')
            ?.querySelector<HTMLElement>('.fm-context-menu-open-with button')
            ?.focus();
      }
      return;
    }
    focusApplication = fromKeyboard;
    submenu = 'loading';
    const generation = ++loadGeneration;
    void attrs.openWithSubmenu
      .load()
      .then((apps) => {
        if (generation !== loadGeneration) return;
        applications = apps;
        submenu = 'ready';
        m.redraw();
        for (const app of apps) {
          void attrs.openWithSubmenu
            ?.iconFor(app.path)
            .then((bytes) => {
              if (generation !== loadGeneration || bytes === undefined) return;
              icons.set(app.path, iconDataUri(bytes));
              m.redraw();
            })
            .catch((error: unknown) => {
              console.warn(`Could not load application icon for ${app.name}`, error);
            });
        }
      })
      .catch((error: unknown) => {
        if (generation !== loadGeneration) return;
        console.error('Failed to load Open With applications', error);
        submenu = 'error';
        m.redraw();
      });
  }

  function dismissSubmenu(): void {
    ++loadGeneration;
    applications = [];
    icons.clear();
    submenu = 'closed';
    focusApplication = false;
  }

  function close(attrs: ContextMenuAttrs): void {
    dismissSubmenu();
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
    if (item.action.id === 'core.openWith' && attrs.openWithSubmenu !== undefined) {
      showOpenWith(attrs, true);
      return;
    }
    attrs.onInvoke(item.action.id);
    close(attrs);
  }

  return {
    onupdate: ({ attrs }) => {
      if (attrs.open && previousFocus === undefined)
        previousFocus = document.activeElement as HTMLElement;
    },
    view: ({ attrs }) => {
      if (!attrs.open) {
        dismissSubmenu();
        return undefined;
      }
      const itemCount = attrs.actions.length + (attrs.platformSubmenu === undefined ? 0 : 1);
      activeIndex = Math.min(activeIndex, Math.max(0, itemCount - 1));
      const menuItems: m.Vnode[] = [];
      attrs.actions.forEach((item, index) => {
        const previous = attrs.actions[index - 1];
        if (
          previous !== undefined &&
          contextMenuGroup(previous.action.id) !== contextMenuGroup(item.action.id)
        ) {
          menuItems.push(
            m('.fm-context-menu-separator', {
              key: `separator-${item.action.id}`,
              role: 'separator',
            }),
          );
        }
        const shortcut = item.action.defaultShortcuts[0];
        menuItems.push(
          m(
            'button.fm-context-menu-item',
            {
              key: item.action.id,
              type: 'button',
              role: 'menuitem',
              class: [
                DESTRUCTIVE_CONTEXT_ACTION_IDS.has(item.action.id)
                  ? 'fm-context-menu-item-destructive'
                  : '',
                item.action.id === 'core.openWith' && attrs.openWithSubmenu !== undefined
                  ? 'fm-context-menu-submenu'
                  : '',
                index === activeIndex ? 'fm-context-menu-item-active' : '',
              ]
                .filter(Boolean)
                .join(' '),
              disabled: !item.available,
              tabindex: index === activeIndex ? 0 : -1,
              title: item.reason,
              ...(item.action.id === 'core.openWith' && attrs.openWithSubmenu !== undefined
                ? {
                    'aria-haspopup': 'menu',
                    'aria-expanded': submenu === 'closed' ? 'false' : 'true',
                    'aria-controls': 'fm-open-with-submenu',
                    onmouseenter: (event: MouseEvent) => {
                      openWithItem = event.currentTarget as HTMLElement;
                      if (item.available) showOpenWith(attrs);
                    },
                    oncreate: ({ dom }: m.VnodeDOM) => {
                      openWithItem = dom as HTMLElement;
                    },
                  }
                : {
                    onmouseenter: () => {
                      if (submenu !== 'closed') dismissSubmenu();
                    },
                  }),
              onclick: () => invoke(attrs, index),
            },
            [
              m('span.fm-context-menu-label', item.action.title),
              shortcut === undefined
                ? undefined
                : m(
                    'kbd.fm-context-menu-shortcut',
                    { 'aria-hidden': 'true' },
                    formatShortcut(shortcut, attrs.platform),
                  ),
            ],
          ),
        );
      });
      if (attrs.platformSubmenu !== undefined) {
        menuItems.push(
          m('.fm-context-menu-separator', { key: 'separator-platform', role: 'separator' }),
          m(
            'button.fm-context-menu-item.fm-context-menu-submenu',
            {
              class:
                attrs.actions.length === activeIndex ? 'fm-context-menu-item-active' : undefined,
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
              } else if (
                event.key === 'ArrowRight' &&
                attrs.actions[activeIndex]?.action.id === 'core.openWith'
              ) {
                event.preventDefault();
                showOpenWith(attrs, true);
              }
            },
          },
          menuItems,
        ),
        submenu === 'closed' || attrs.openWithSubmenu === undefined
          ? undefined
          : m(
              '.fm-context-menu.fm-context-menu-open-with',
              {
                id: 'fm-open-with-submenu',
                role: 'menu',
                'aria-label': attrs.actions.find((item) => item.action.id === 'core.openWith')
                  ?.action.title,
                oncreate: ({ dom }) => positionSubmenu(dom as HTMLElement, openWithItem),
                onupdate: ({ dom }) => {
                  positionSubmenu(dom as HTMLElement, openWithItem);
                  if (focusApplication && submenu !== 'loading') {
                    (dom as HTMLElement).querySelector<HTMLButtonElement>('button')?.focus();
                    focusApplication = false;
                  }
                },
                onclick: (event: MouseEvent) => event.stopPropagation(),
                onkeydown: (event: KeyboardEvent) => {
                  if (event.key === 'ArrowLeft') {
                    event.preventDefault();
                    dismissSubmenu();
                    openWithItem?.focus();
                  } else if (event.key === 'Escape') {
                    event.preventDefault();
                    close(attrs);
                  } else if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
                    event.preventDefault();
                    const items = [
                      ...(event.currentTarget as HTMLElement).querySelectorAll<HTMLButtonElement>(
                        'button',
                      ),
                    ];
                    const index = items.indexOf(document.activeElement as HTMLButtonElement);
                    items[
                      (index + (event.key === 'ArrowDown' ? 1 : -1) + items.length) % items.length
                    ]?.focus();
                  }
                },
              },
              [
                submenu === 'loading'
                  ? m('.fm-context-menu-message', t('contextMenu', 'loadingApplications'))
                  : submenu === 'error'
                    ? m('.fm-context-menu-message', t('contextMenu', 'applicationsFailed'))
                    : applications.map((app) =>
                        m(
                          'button.fm-context-menu-item',
                          {
                            key: app.path,
                            type: 'button',
                            role: 'menuitem',
                            onclick: () => {
                              attrs.openWithSubmenu?.onChoose(app.path);
                              close(attrs);
                            },
                          },
                          [
                            icons.has(app.path)
                              ? m('img.fm-context-menu-app-icon', {
                                  src: icons.get(app.path),
                                  alt: '',
                                })
                              : m('span.fm-context-menu-app-icon'),
                            m('span.fm-context-menu-label', app.name),
                          ],
                        ),
                      ),
                m('.fm-context-menu-separator', { role: 'separator' }),
                m(
                  'button.fm-context-menu-item',
                  {
                    type: 'button',
                    role: 'menuitem',
                    onclick: () => {
                      attrs.openWithSubmenu?.onOther();
                      close(attrs);
                    },
                  },
                  t('contextMenu', 'otherApplications'),
                ),
              ],
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

function positionSubmenu(menu: HTMLElement, trigger?: HTMLElement): void {
  if (trigger === undefined) return;
  const rect = trigger.getBoundingClientRect();
  const width = menu.getBoundingClientRect().width;
  menu.style.left = `${rect.right + width + 8 > window.innerWidth ? rect.left - width : rect.right}px`;
  menu.style.top = `${Math.max(8, Math.min(rect.top, window.innerHeight - menu.offsetHeight - 8))}px`;
}
