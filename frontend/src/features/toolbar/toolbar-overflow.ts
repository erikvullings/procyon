import m, { type Children, type Vnode } from 'mithril';
import { IconButton } from 'mithril-materialized';
import { dotsIcon } from '../../components/tabler-icons.ts';

/** One entry of the toolbar's overflow menu. */
export interface ToolbarOverflowItem {
  readonly id: string;
  readonly label: string;
  /** Display form of the action's shortcut, shown right-aligned like every other menu. */
  readonly shortcut?: string | undefined;
  readonly icon: Children;
  readonly disabled?: boolean;
  /** Set for toggles (e.g. the basket tab being open). */
  readonly pressed?: boolean;
  readonly onclick: () => void;
}

export interface ToolbarOverflowAttrs {
  readonly label: string;
  readonly open: boolean;
  readonly items: readonly ToolbarOverflowItem[];
  readonly onToggle: (open: boolean) => void;
}

/**
 * The compact toolbar's "more" button and its menu. Everything that is not navigation, search,
 * workspaces or the command palette lives here, so the window chrome stays light while every
 * action stays one click away.
 */
export const ToolbarOverflow = {
  view: ({ attrs }: Vnode<ToolbarOverflowAttrs>): Children =>
    m(
      '.fm-toolbar-overflow',
      {
        onkeydown: (event: KeyboardEvent) => {
          if (event.key !== 'Escape' || !attrs.open) return;
          event.preventDefault();
          attrs.onToggle(false);
        },
      },
      [
        m(
          IconButton,
          {
            className: 'fm-toolbar-overflow-button',
            'aria-label': attrs.label,
            'aria-haspopup': 'menu',
            'aria-expanded': String(attrs.open),
            onclick: () => attrs.onToggle(!attrs.open),
          },
          dotsIcon(),
        ),
        attrs.open
          ? [
              m('.fm-toolbar-overflow-backdrop', { onclick: () => attrs.onToggle(false) }),
              m(
                '.fm-toolbar-overflow-menu',
                { role: 'menu', 'aria-label': attrs.label },
                attrs.items.map((item) =>
                  m(
                    'button.fm-toolbar-overflow-item',
                    {
                      key: item.id,
                      type: 'button',
                      role: item.pressed === undefined ? 'menuitem' : 'menuitemcheckbox',
                      'aria-checked': item.pressed === undefined ? undefined : String(item.pressed),
                      disabled: item.disabled === true,
                      onclick: () => {
                        attrs.onToggle(false);
                        item.onclick();
                      },
                    },
                    [
                      m('span.fm-toolbar-overflow-icon', item.icon),
                      m('span.fm-toolbar-overflow-label', item.label),
                      item.shortcut === undefined
                        ? undefined
                        : m('span.fm-menu-shortcut', item.shortcut),
                    ],
                  ),
                ),
              ),
            ]
          : undefined,
      ],
    ),
};
