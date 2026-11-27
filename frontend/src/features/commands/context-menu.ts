import m, { type FactoryComponent } from 'mithril';

import type { OpenWithApplication } from '../../api/client/file-manager-client';
import { t } from '../../i18n';
import type { SelectionPlatform } from '../selection/keybindings';
import {
  type AvailableAction,
  CONTEXT_NEW_ACTION_IDS,
  type ContextSubmenuId,
  contextMenuGroup,
  contextSubmenuFor,
  DESTRUCTIVE_CONTEXT_ACTION_IDS,
} from './availability';
import { formatShortcut } from './shortcut-label';

/** One template offered by the "New" submenu (task 0312). */
export interface NewMenuChoice {
  readonly id: string;
  readonly label: string;
  /** Where it comes from, for people with several template sources; absent for app templates. */
  readonly detail?: string;
}

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
  /**
   * The "New" submenu of the empty-area menu (task 0312): a new folder, then the templates. It
   * stands in for the create-directory and new-from-template entries of `actions`.
   */
  readonly newSubmenu?: {
    readonly title: string;
    readonly load: () => Promise<readonly NewMenuChoice[]>;
    readonly onChoose: (id: string) => void;
  };
  readonly onClose: () => void;
  readonly onInvoke: (actionId: string) => void;
  readonly placementBounds?: () => DOMRect | undefined;
}

const CONTEXT_MENU_VIEWPORT_MARGIN = 8;

/** Actions the "New" submenu replaces in the menu (task 0312). */
const NEW_SUBMENU_ACTION_IDS: ReadonlySet<string> = CONTEXT_NEW_ACTION_IDS;

/** Titles of the grouped submenus (task 0312 follow-up). */
const SUBMENU_TITLE_KEY: Record<
  ContextSubmenuId,
  'submenuMedia' | 'submenuArchive' | 'submenuTools'
> = {
  media: 'submenuMedia',
  archive: 'submenuArchive',
  tools: 'submenuTools',
};

/** One row of the menu, in the order it is shown and navigated. */
type MenuRow =
  | { readonly kind: 'action'; readonly index: number }
  | { readonly kind: 'new' }
  | { readonly kind: 'group'; readonly id: ContextSubmenuId; readonly indexes: readonly number[] }
  | { readonly kind: 'platform' };

function menuRows(attrs: ContextMenuAttrs): MenuRow[] {
  // Actions the selection can never match are left out; the others of one submenu share a row.
  const visible = attrs.actions.flatMap((item, index) => (item.hidden ? [] : [{ item, index }]));
  const groups = new Map<ContextSubmenuId, number[]>();
  for (const { item, index } of visible) {
    const id = contextSubmenuFor(item.action.id);
    if (id !== undefined) groups.set(id, [...(groups.get(id) ?? []), index]);
  }
  const rows: MenuRow[] = [];
  for (const { item, index } of visible) {
    if (attrs.newSubmenu !== undefined && NEW_SUBMENU_ACTION_IDS.has(item.action.id)) {
      // The first of them stands for the whole submenu.
      if (!rows.some((row) => row.kind === 'new')) rows.push({ kind: 'new' });
      continue;
    }
    const id = contextSubmenuFor(item.action.id);
    if (id === undefined) {
      rows.push({ kind: 'action', index });
    } else if (!rows.some((row) => row.kind === 'group' && row.id === id)) {
      rows.push({ kind: 'group', id, indexes: groups.get(id) ?? [] });
    }
  }
  if (attrs.platformSubmenu !== undefined) rows.push({ kind: 'platform' });
  return rows;
}

/** The indexes of the visible actions of one grouped submenu, in menu order. */
function groupMembers(attrs: ContextMenuAttrs, id: ContextSubmenuId): number[] {
  return attrs.actions.flatMap((item, index) =>
    !item.hidden && contextSubmenuFor(item.action.id) === id ? [index] : [],
  );
}

function newSubmenuReason(attrs: ContextMenuAttrs): string | undefined {
  return attrs.actions.find((item) => NEW_SUBMENU_ACTION_IDS.has(item.action.id))?.reason;
}

function newSubmenuAvailable(attrs: ContextMenuAttrs): boolean {
  return attrs.actions.some((item) => NEW_SUBMENU_ACTION_IDS.has(item.action.id) && item.available);
}

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
  let newFlyout: 'closed' | 'loading' | 'ready' | 'error' = 'closed';
  let openGroup: ContextSubmenuId | undefined;
  let groupItem: HTMLElement | undefined;
  let groupElement: HTMLElement | undefined;
  let focusGroup = false;
  let newChoices: readonly NewMenuChoice[] = [];
  let newGeneration = 0;
  let newItem: HTMLElement | undefined;
  let newElement: HTMLElement | undefined;
  let focusNew = false;
  let menuElement: HTMLElement | undefined;
  let submenuElement: HTMLElement | undefined;
  let currentAttrs: ContextMenuAttrs;

  function closeOnOutsideClick(event: MouseEvent): void {
    if (
      event.target instanceof Node &&
      (menuElement?.contains(event.target) ||
        submenuElement?.contains(event.target) ||
        newElement?.contains(event.target) ||
        groupElement?.contains(event.target))
    )
      return;
    close(currentAttrs);
    m.redraw();
  }

  function preventNativeContextMenu(event: MouseEvent): void {
    event.preventDefault();
  }

  function repositionOnResize(): void {
    if (menuElement === undefined) return;
    positionMenu(menuElement, currentAttrs);
    if (submenuElement !== undefined) positionSubmenu(submenuElement, openWithItem, currentAttrs);
    if (newElement !== undefined) positionSubmenu(newElement, newItem, currentAttrs);
    if (groupElement !== undefined) positionSubmenu(groupElement, groupItem, currentAttrs);
  }

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

  /** Opens the "New" flyout and loads the templates (task 0312). */
  function showNew(attrs: ContextMenuAttrs, fromKeyboard = false): void {
    if (attrs.newSubmenu === undefined || !newSubmenuAvailable(attrs)) return;
    dismissGroup();
    if (newFlyout !== 'closed') {
      if (fromKeyboard) newElement?.querySelector<HTMLElement>('button')?.focus();
      return;
    }
    focusNew = fromKeyboard;
    newFlyout = 'loading';
    const generation = ++newGeneration;
    void attrs.newSubmenu
      .load()
      .then((choices) => {
        if (generation !== newGeneration) return;
        newChoices = choices;
        newFlyout = 'ready';
        m.redraw();
      })
      .catch((error: unknown) => {
        if (generation !== newGeneration) return;
        console.error('Failed to load file templates for the New submenu', error);
        newFlyout = 'error';
        m.redraw();
      });
  }

  function dismissNew(): void {
    ++newGeneration;
    newChoices = [];
    newFlyout = 'closed';
    focusNew = false;
    newElement = undefined;
  }

  /** Opens a grouped submenu's flyout (Media, Archive, Tools). */
  function showGroup(attrs: ContextMenuAttrs, id: ContextSubmenuId, fromKeyboard = false): void {
    if (openGroup === id) {
      if (fromKeyboard) groupElement?.querySelector<HTMLElement>('button:not(:disabled)')?.focus();
      return;
    }
    dismissSubmenu();
    dismissNew();
    focusGroup = fromKeyboard;
    openGroup = id;
    m.redraw();
    void attrs;
  }

  function dismissGroup(): void {
    openGroup = undefined;
    focusGroup = false;
    groupElement = undefined;
  }

  function dismissSubmenu(): void {
    ++loadGeneration;
    applications = [];
    icons.clear();
    submenu = 'closed';
    focusApplication = false;
    submenuElement = undefined;
  }

  function close(attrs: ContextMenuAttrs): void {
    dismissSubmenu();
    dismissNew();
    dismissGroup();
    attrs.onClose();
    previousFocus?.focus();
    previousFocus = undefined;
  }

  function invoke(attrs: ContextMenuAttrs, row: MenuRow): void {
    if (row.kind === 'platform' && attrs.platformSubmenu !== undefined) {
      attrs.platformSubmenu.onOpen();
      close(attrs);
      return;
    }
    if (row.kind === 'new') {
      showNew(attrs, true);
      return;
    }
    if (row.kind === 'group') {
      showGroup(attrs, row.id, true);
      return;
    }
    if (row.kind !== 'action') return;
    const item = attrs.actions[row.index];
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
      currentAttrs = attrs;
      if (!attrs.open) {
        dismissSubmenu();
        dismissNew();
        dismissGroup();
        return undefined;
      }
      const rows = menuRows(attrs);
      const itemCount = rows.length;
      activeIndex = Math.min(activeIndex, Math.max(0, itemCount - 1));
      const menuItems: m.Vnode[] = [];
      let previousGroup: number | undefined;
      rows.forEach((row, rowIndex) => {
        if (row.kind === 'platform') {
          menuItems.push(
            m('.fm-context-menu-separator', { key: 'separator-platform', role: 'separator' }),
            m(
              'button.fm-context-menu-item.fm-context-menu-submenu',
              {
                class: rowIndex === activeIndex ? 'fm-context-menu-item-active' : undefined,
                key: 'platform-submenu',
                type: 'button',
                role: 'menuitem',
                'aria-haspopup': 'menu',
                tabindex: rowIndex === activeIndex ? 0 : -1,
                onclick: () => invoke(attrs, row),
              },
              attrs.platformSubmenu?.title,
            ),
          );
          return;
        }
        const groupId =
          row.kind === 'new'
            ? 'core.createDirectory'
            : row.kind === 'group'
              ? (attrs.actions[row.indexes[0] ?? -1]?.action.id ?? '')
              : (attrs.actions[row.index]?.action.id ?? '');
        const group = contextMenuGroup(groupId);
        if (previousGroup !== undefined && previousGroup !== group) {
          menuItems.push(
            m('.fm-context-menu-separator', {
              key: `separator-${groupId}-${rowIndex}`,
              role: 'separator',
            }),
          );
        }
        previousGroup = group;
        if (row.kind === 'group') {
          const available = row.indexes.some((index) => attrs.actions[index]?.available === true);
          menuItems.push(
            m(
              'button.fm-context-menu-item.fm-context-menu-submenu',
              {
                key: `group-${row.id}`,
                type: 'button',
                role: 'menuitem',
                class: rowIndex === activeIndex ? 'fm-context-menu-item-active' : undefined,
                disabled: !available,
                tabindex: rowIndex === activeIndex ? 0 : -1,
                'aria-haspopup': 'menu',
                'aria-expanded': openGroup === row.id ? 'true' : 'false',
                'aria-controls': 'fm-group-submenu',
                oncreate: ({ dom }: m.VnodeDOM) => {
                  if (row.id === openGroup) groupItem = dom as HTMLElement;
                },
                onmouseenter: (event: MouseEvent) => {
                  groupItem = event.currentTarget as HTMLElement;
                  if (available) showGroup(attrs, row.id);
                },
                onclick: () => invoke(attrs, row),
              },
              [m('span.fm-context-menu-label', t('contextMenu', SUBMENU_TITLE_KEY[row.id]))],
            ),
          );
          return;
        }
        if (row.kind === 'new') {
          const available = newSubmenuAvailable(attrs);
          menuItems.push(
            m(
              'button.fm-context-menu-item.fm-context-menu-submenu',
              {
                key: 'new-submenu',
                type: 'button',
                role: 'menuitem',
                class: rowIndex === activeIndex ? 'fm-context-menu-item-active' : undefined,
                disabled: !available,
                tabindex: rowIndex === activeIndex ? 0 : -1,
                'aria-haspopup': 'menu',
                'aria-expanded': newFlyout === 'closed' ? 'false' : 'true',
                'aria-controls': 'fm-new-submenu',
                title: available ? undefined : newSubmenuReason(attrs),
                oncreate: ({ dom }: m.VnodeDOM) => {
                  newItem = dom as HTMLElement;
                },
                onmouseenter: (event: MouseEvent) => {
                  newItem = event.currentTarget as HTMLElement;
                  if (available) showNew(attrs);
                },
                onclick: () => invoke(attrs, row),
              },
              [m('span.fm-context-menu-label', attrs.newSubmenu?.title)],
            ),
          );
          return;
        }
        const item = attrs.actions[row.index];
        if (item === undefined) return;
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
                rowIndex === activeIndex ? 'fm-context-menu-item-active' : '',
              ]
                .filter(Boolean)
                .join(' '),
              disabled: !item.available,
              tabindex: rowIndex === activeIndex ? 0 : -1,
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
                      if (newFlyout !== 'closed') dismissNew();
                      if (openGroup !== undefined) dismissGroup();
                    },
                  }),
              onclick: () => invoke(attrs, row),
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
      return m('.fm-context-menu-backdrop', { style: { pointerEvents: 'none' } }, [
        m(
          '.fm-context-menu',
          {
            role: 'menu',
            tabindex: -1,
            'aria-label': t('contextMenu', 'directoryActions'),
            style: { left: `${attrs.x}px`, top: `${attrs.y}px`, pointerEvents: 'auto' },
            oncreate: ({ dom }) => {
              menuElement = dom as HTMLElement;
              window.addEventListener('resize', repositionOnResize);
              document.addEventListener('click', closeOnOutsideClick, true);
              document.addEventListener('contextmenu', preventNativeContextMenu, true);
              positionMenu(dom as HTMLElement, attrs);
              if (previousFocus === undefined)
                previousFocus = document.activeElement as HTMLElement;
              (dom as HTMLElement).focus();
            },
            onupdate: ({ dom }) => {
              positionMenu(dom as HTMLElement, attrs);
              const focused = document.activeElement;
              if (
                focused !== null &&
                !dom.contains(focused) &&
                !submenuElement?.contains(focused) &&
                !newElement?.contains(focused) &&
                !groupElement?.contains(focused)
              )
                (dom as HTMLElement).focus();
            },
            onremove: () => {
              window.removeEventListener('resize', repositionOnResize);
              document.removeEventListener('click', closeOnOutsideClick, true);
              document.removeEventListener('contextmenu', preventNativeContextMenu, true);
              menuElement = undefined;
              submenuElement = undefined;
              newElement = undefined;
            },
            onclick: (event: MouseEvent) => event.stopPropagation(),
            onkeydown: (event: KeyboardEvent) => {
              const row = rows[activeIndex];
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
                if (row !== undefined) invoke(attrs, row);
              } else if (event.key === 'ArrowRight' && row?.kind === 'new') {
                event.preventDefault();
                showNew(attrs, true);
              } else if (event.key === 'ArrowRight' && row?.kind === 'group') {
                event.preventDefault();
                showGroup(attrs, row.id, true);
              } else if (
                event.key === 'ArrowRight' &&
                row?.kind === 'action' &&
                attrs.actions[row.index]?.action.id === 'core.openWith'
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
                style: { pointerEvents: 'auto' },
                'aria-label': attrs.actions.find((item) => item.action.id === 'core.openWith')
                  ?.action.title,
                oncreate: ({ dom }) => {
                  submenuElement = dom as HTMLElement;
                  positionSubmenu(dom as HTMLElement, openWithItem, attrs);
                },
                onupdate: ({ dom }) => {
                  positionSubmenu(dom as HTMLElement, openWithItem, attrs);
                  if (focusApplication && submenu !== 'loading') {
                    (dom as HTMLElement).querySelector<HTMLButtonElement>('button')?.focus();
                    focusApplication = false;
                  }
                },
                onremove: () => {
                  submenuElement = undefined;
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
        openGroup === undefined
          ? undefined
          : m(
              '.fm-context-menu.fm-context-menu-open-with',
              {
                id: 'fm-group-submenu',
                role: 'menu',
                style: { pointerEvents: 'auto' },
                'aria-label': t('contextMenu', SUBMENU_TITLE_KEY[openGroup]),
                oncreate: ({ dom }) => {
                  groupElement = dom as HTMLElement;
                  positionSubmenu(dom as HTMLElement, groupItem, attrs);
                },
                onupdate: ({ dom }) => {
                  positionSubmenu(dom as HTMLElement, groupItem, attrs);
                  if (focusGroup) {
                    (dom as HTMLElement)
                      .querySelector<HTMLButtonElement>('button:not(:disabled)')
                      ?.focus();
                    focusGroup = false;
                  }
                },
                onremove: () => {
                  groupElement = undefined;
                },
                onclick: (event: MouseEvent) => event.stopPropagation(),
                onkeydown: (event: KeyboardEvent) => {
                  if (event.key === 'ArrowLeft') {
                    event.preventDefault();
                    dismissGroup();
                    groupItem?.focus();
                  } else if (event.key === 'Escape') {
                    event.preventDefault();
                    close(attrs);
                  } else if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
                    event.preventDefault();
                    const items = [
                      ...(event.currentTarget as HTMLElement).querySelectorAll<HTMLButtonElement>(
                        'button:not(:disabled)',
                      ),
                    ];
                    const index = items.indexOf(document.activeElement as HTMLButtonElement);
                    items[
                      (index + (event.key === 'ArrowDown' ? 1 : -1) + items.length) % items.length
                    ]?.focus();
                  }
                },
              },
              groupMembers(attrs, openGroup).map((index) => {
                const item = attrs.actions[index];
                const shortcut = item?.action.defaultShortcuts[0];
                return m(
                  'button.fm-context-menu-item',
                  {
                    key: item?.action.id,
                    type: 'button',
                    role: 'menuitem',
                    disabled: item?.available !== true,
                    title: item?.reason,
                    onclick: () => {
                      if (item === undefined || !item.available) return;
                      attrs.onInvoke(item.action.id);
                      close(attrs);
                    },
                  },
                  [
                    m('span.fm-context-menu-label', item?.action.title),
                    shortcut === undefined
                      ? undefined
                      : m(
                          'kbd.fm-context-menu-shortcut',
                          { 'aria-hidden': 'true' },
                          formatShortcut(shortcut, attrs.platform),
                        ),
                  ],
                );
              }),
            ),
        newFlyout === 'closed' || attrs.newSubmenu === undefined
          ? undefined
          : m(
              '.fm-context-menu.fm-context-menu-open-with',
              {
                id: 'fm-new-submenu',
                role: 'menu',
                style: { pointerEvents: 'auto' },
                'aria-label': attrs.newSubmenu.title,
                oncreate: ({ dom }) => {
                  newElement = dom as HTMLElement;
                  positionSubmenu(dom as HTMLElement, newItem, attrs);
                },
                onupdate: ({ dom }) => {
                  positionSubmenu(dom as HTMLElement, newItem, attrs);
                  if (focusNew && newFlyout !== 'loading') {
                    (dom as HTMLElement).querySelector<HTMLButtonElement>('button')?.focus();
                    focusNew = false;
                  }
                },
                onremove: () => {
                  newElement = undefined;
                },
                onclick: (event: MouseEvent) => event.stopPropagation(),
                onkeydown: (event: KeyboardEvent) => {
                  if (event.key === 'ArrowLeft') {
                    event.preventDefault();
                    dismissNew();
                    newItem?.focus();
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
                newFlyout === 'loading'
                  ? m('.fm-context-menu-message', t('contextMenu', 'loadingTemplates'))
                  : newFlyout === 'error'
                    ? m('.fm-context-menu-message', t('newFromTemplate', 'loadFailed'))
                    : undefined,
                m(
                  'button.fm-context-menu-item',
                  {
                    type: 'button',
                    role: 'menuitem',
                    onclick: () => {
                      attrs.onInvoke('core.createDirectory');
                      close(attrs);
                    },
                  },
                  [
                    m(
                      'span.fm-context-menu-label',
                      attrs.actions.find((item) => item.action.id === 'core.createDirectory')
                        ?.action.title,
                    ),
                  ],
                ),
                newFlyout === 'ready' && newChoices.length > 0
                  ? m('.fm-context-menu-separator', { role: 'separator' })
                  : undefined,
                newChoices.map((choice) =>
                  m(
                    'button.fm-context-menu-item',
                    {
                      key: choice.id,
                      type: 'button',
                      role: 'menuitem',
                      onclick: () => {
                        attrs.newSubmenu?.onChoose(choice.id);
                        close(attrs);
                      },
                    },
                    [
                      m('span.fm-context-menu-label', choice.label),
                      choice.detail === undefined
                        ? undefined
                        : m('span.fm-context-menu-shortcut', choice.detail),
                    ],
                  ),
                ),
              ],
            ),
      ]);
    },
  };
};

function positionMenu(menu: HTMLElement, attrs: ContextMenuAttrs): void {
  const bounds = attrs.placementBounds?.();
  menu.style.minWidth = bounds === undefined ? '' : '0';
  menu.style.maxWidth = bounds === undefined ? '' : `${Math.max(0, bounds.width - 16)}px`;
  menu.style.maxHeight = bounds === undefined ? '' : `${Math.max(0, bounds.height - 16)}px`;
  menu.style.overflowY = bounds === undefined ? '' : 'auto';
  const rect = menu.getBoundingClientRect();
  const position =
    bounds === undefined
      ? clampContextMenuPosition(
          attrs.x,
          attrs.y,
          rect.width,
          rect.height,
          window.innerWidth,
          window.innerHeight,
        )
      : {
          x: Math.max(bounds.left + 8, Math.min(attrs.x, bounds.right - rect.width - 8)),
          y: Math.max(bounds.top + 8, Math.min(attrs.y, bounds.bottom - rect.height - 8)),
        };
  menu.style.left = `${position.x}px`;
  menu.style.top = `${position.y}px`;
}

function positionSubmenu(
  menu: HTMLElement,
  trigger: HTMLElement | undefined,
  attrs: ContextMenuAttrs,
): void {
  if (trigger === undefined) return;
  const bounds = attrs.placementBounds?.();
  menu.style.minWidth = bounds === undefined ? '' : '0';
  menu.style.maxWidth = bounds === undefined ? '' : `${Math.max(0, bounds.width - 16)}px`;
  menu.style.maxHeight = bounds === undefined ? '' : `${Math.max(0, bounds.height - 16)}px`;
  const rect = trigger.getBoundingClientRect();
  const width = menu.getBoundingClientRect().width;
  const left = bounds?.left ?? 0;
  const right = bounds?.right ?? window.innerWidth;
  const top = bounds?.top ?? 0;
  const bottom = bounds?.bottom ?? window.innerHeight;
  const x = rect.right + width + 8 <= right ? rect.right : rect.left - width;
  menu.style.left = `${Math.max(left + 8, Math.min(x, right - width - 8))}px`;
  menu.style.top = `${Math.max(top + 8, Math.min(rect.top, bottom - menu.offsetHeight - 8))}px`;
}
