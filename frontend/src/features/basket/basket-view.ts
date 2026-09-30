import m, { type FactoryComponent } from 'mithril';
import { t } from '../../i18n';
import type { Location } from '../../models';
import type { BasketState } from './basket';

const ROW_HEIGHT = 48;

export interface BasketViewAttrs {
  readonly basket: BasketState;
  readonly destination?: Location;
  readonly busy: boolean;
  readonly onToggle: (key: string) => void;
  readonly onRemove: (key: string) => void;
  readonly onClear: () => void;
  readonly onPersist: (enabled: boolean) => void;
  readonly onRefresh: () => void;
  readonly onAction: (kind: 'copy' | 'move' | 'checksum' | 'archive' | 'delete') => void;
}

/** Virtualized collection list; the native checkboxes/buttons provide keyboard and screen-reader access. */
export const BasketView: FactoryComponent<BasketViewAttrs> = () => {
  let scrollTop = 0;
  let viewportHeight = 600;
  let observer: ResizeObserver | undefined;
  return {
    onremove: () => observer?.disconnect(),
    view: ({ attrs }) => {
      const { basket } = attrs;
      const start = Math.max(0, Math.floor(scrollTop / ROW_HEIGHT) - 4);
      const end = Math.min(basket.items.length, start + Math.ceil(viewportHeight / ROW_HEIGHT) + 8);
      const selected = new Set(basket.selectedKeys);
      const actionable = basket.items.some(
        (item) => item.status === 'ready' && (selected.size === 0 || selected.has(item.key)),
      );
      return m('.fm-basket', [
        m('.fm-basket-header', [
          m('h2', t('basket', 'title')),
          m('span', t('basket', 'count', basket.items.length)),
          m(
            'button',
            { type: 'button', onclick: attrs.onRefresh, disabled: attrs.busy },
            t('basket', 'check'),
          ),
          m(
            'button',
            { type: 'button', onclick: attrs.onClear, disabled: basket.items.length === 0 },
            t('basket', 'clear'),
          ),
        ]),
        m('label.fm-basket-persist', [
          m('input', {
            type: 'checkbox',
            checked: basket.persist,
            onchange: (event: Event) =>
              attrs.onPersist((event.currentTarget as HTMLInputElement).checked),
          }),
          m('span', t('basket', 'persist')),
        ]),
        m('.fm-basket-hint', t('basket', 'hint')),
        m(
          '.fm-basket-list',
          {
            role: 'list',
            'aria-label': t('basket', 'list'),
            oncreate: ({ dom }) => {
              const element = dom as HTMLElement;
              viewportHeight = element.clientHeight || viewportHeight;
              if (typeof ResizeObserver !== 'undefined') {
                observer = new ResizeObserver(() => {
                  viewportHeight = element.clientHeight || viewportHeight;
                  m.redraw();
                });
                observer.observe(element);
              }
            },
            onscroll: (event: Event) => {
              scrollTop = (event.currentTarget as HTMLElement).scrollTop;
            },
          },
          basket.items.length === 0
            ? m('p.fm-basket-empty', t('basket', 'empty'))
            : m(
                '.fm-basket-spacer',
                { style: { height: `${basket.items.length * ROW_HEIGHT}px` } },
                basket.items.slice(start, end).map((item, offset) =>
                  m(
                    '.fm-basket-row',
                    {
                      key: item.key,
                      role: 'listitem',
                      style: { top: `${(start + offset) * ROW_HEIGHT}px` },
                    },
                    [
                      m('label.fm-basket-select', [
                        m('input', {
                          type: 'checkbox',
                          checked: selected.has(item.key),
                          'aria-label': t('basket', 'select', { name: item.name }),
                          onchange: () => attrs.onToggle(item.key),
                        }),
                        m('span'),
                      ]),
                      m('.fm-basket-entry', [
                        m('strong', item.name),
                        m(
                          'small',
                          { title: item.location.uri },
                          `${item.location.providerId} · ${item.location.uri}`,
                        ),
                      ]),
                      item.status === 'ready'
                        ? undefined
                        : m(
                            'span.fm-basket-warning',
                            {
                              role: 'status',
                            },
                            item.status === 'missing'
                              ? t('basket', 'missing')
                              : item.status === 'moved'
                                ? t('basket', 'moved')
                                : t('basket', 'stale'),
                          ),
                      m(
                        'button',
                        {
                          type: 'button',
                          'aria-label': t('basket', 'remove', { name: item.name }),
                          onclick: () => attrs.onRemove(item.key),
                        },
                        t('pane', 'remove'),
                      ),
                    ],
                  ),
                ),
              ),
        ),
        m('.fm-basket-actions', [
          attrs.destination === undefined
            ? m('span', t('basket', 'noDestination'))
            : m(
                'span',
                { title: attrs.destination.uri },
                t('basket', 'destination', { uri: attrs.destination.uri }),
              ),
          ...(['copy', 'move', 'checksum', 'archive', 'delete'] as const).map((kind) =>
            m(
              'button',
              {
                type: 'button',
                disabled:
                  attrs.busy ||
                  !actionable ||
                  ((kind === 'copy' || kind === 'move' || kind === 'archive') &&
                    attrs.destination === undefined),
                onclick: () => attrs.onAction(kind),
              },
              kind === 'checksum'
                ? t('checksums', 'title')
                : kind === 'archive'
                  ? t('archiveCreate', 'createTitle')
                  : kind === 'delete'
                    ? t('button', 'delete')
                    : t('operation', kind),
            ),
          ),
        ]),
      ]);
    },
  };
};
