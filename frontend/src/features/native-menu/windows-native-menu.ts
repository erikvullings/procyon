import m, { type FactoryComponent } from 'mithril';
import { t } from '../../i18n';
import type { NativeMenuItem, NativeMenuRole, NativeMenuSpec } from '../../models';

export interface WindowsNativeMenuAttrs {
  readonly spec: NativeMenuSpec;
  readonly onAction: (id: string) => void;
  readonly onRole: (role: NativeMenuRole) => void;
}

function itemLabel(item: NativeMenuItem): string {
  if (item.kind === 'action' || item.kind === 'submenu') return item.title;
  if (item.kind === 'role') {
    const labels: Record<NativeMenuRole, string> = {
      about: t('menu', 'about'),
      services: t('menu', 'services'),
      hideApp: t('menu', 'hide'),
      hideOthers: t('menu', 'hideOthers'),
      showAll: t('menu', 'showAll'),
      quit: t('menu', 'exit'),
      minimize: t('menu', 'minimize'),
      zoom: t('menu', 'maximize'),
      bringAllToFront: t('menu', 'bringAllToFront'),
    };
    return labels[item.role];
  }
  return '';
}

export const WindowsNativeMenu: FactoryComponent<WindowsNativeMenuAttrs> = () => {
  let openMenu: number | undefined;
  let menuBar: HTMLElement | undefined;

  function closeMenu(): void {
    if (openMenu === undefined) return;
    openMenu = undefined;
    m.redraw();
  }

  function renderItem(item: NativeMenuItem): m.Children {
    if (item.kind === 'separator')
      return m('li.fm-windows-native-menu-separator', { role: 'separator' });
    if (item.kind === 'submenu') {
      return m('li.fm-windows-native-menu-submenu', [
        m('span', item.title),
        m('ul.fm-windows-native-menu-popup', { role: 'menu' }, item.items.map(renderItem)),
      ]);
    }
    return m(
      'li.fm-windows-native-menu-item',
      {
        role: 'menuitem',
        class: item.kind === 'action' && item.enabled === false ? 'is-disabled' : undefined,
        onclick: (event: MouseEvent) => {
          event.stopPropagation();
          if (item.kind === 'action' && item.enabled !== false) {
            closeMenu();
            attrsOnAction(item.id);
          } else if (item.kind === 'role') {
            closeMenu();
            attrsOnRole(item.role);
          }
        },
      },
      [
        m('span', itemLabel(item)),
        item.kind === 'action' && item.shortcut !== undefined
          ? m('kbd', shortcutLabel(item.shortcut))
          : undefined,
      ],
    );
  }

  let attrsOnAction: (id: string) => void = () => undefined;
  let attrsOnRole: (role: NativeMenuRole) => void = () => undefined;

  return {
    oncreate: ({ dom }) => {
      menuBar = dom as HTMLElement;
      document.addEventListener('click', handleDocumentClick, true);
    },
    onremove: () => {
      document.removeEventListener('click', handleDocumentClick, true);
      menuBar = undefined;
    },
    view: ({ attrs }) => {
      attrsOnAction = attrs.onAction;
      attrsOnRole = attrs.onRole;
      return m(
        '.fm-windows-native-menu',
        {
          role: 'menubar',
          onkeydown: (event: KeyboardEvent) => {
            if (event.key === 'Escape') closeMenu();
          },
        },
        attrs.spec.menus.slice(1).map((menu, index) =>
          m('.fm-windows-native-menu-group', [
            m(
              'button.fm-windows-native-menu-trigger',
              {
                type: 'button',
                'aria-haspopup': 'menu',
                'aria-expanded': openMenu === index ? 'true' : 'false',
                onclick: (event: MouseEvent) => {
                  event.stopPropagation();
                  openMenu = openMenu === index ? undefined : index;
                },
              },
              menu.title,
            ),
            openMenu === index
              ? m('ul.fm-windows-native-menu-popup', { role: 'menu' }, menu.items.map(renderItem))
              : undefined,
          ]),
        ),
      );
    },
  };

  function handleDocumentClick(event: MouseEvent): void {
    if (menuBar !== undefined && !menuBar.contains(event.target as Node)) closeMenu();
  }
};

function shortcutLabel(shortcut: {
  key: string;
  ctrl?: boolean;
  shift?: boolean;
  alt?: boolean;
  meta?: boolean;
}): string {
  const modifiers = [
    shortcut.ctrl || shortcut.meta ? 'Ctrl' : undefined,
    shortcut.alt ? 'Alt' : undefined,
    shortcut.shift ? 'Shift' : undefined,
  ].filter((value): value is string => value !== undefined);
  return [...modifiers, shortcut.key].join('+');
}
