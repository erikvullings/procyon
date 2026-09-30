import m, { type FactoryComponent } from 'mithril';
import { t } from '../../i18n';
import type { Location } from '../../models';
import { formatListingSummary, sizeLabel } from '../panes/pane-status-summary';
import { type BasketState, basketSummary } from './basket';

const ROW_HEIGHT = 20;

export interface BasketViewAttrs {
  readonly basket: BasketState;
  readonly destination?: Location;
  readonly busy: boolean;
  readonly addShortcut?: string;
  readonly onAdd: () => void;
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
      const summary = basketSummary(basket);
      const actionable = basket.items.some(
        (item) => item.status === 'ready' && (selected.size === 0 || selected.has(item.key)),
      );
      return m('.fm-basket', [
        m('.fm-basket-header', [
          m('h2', t('basket', 'title')),
          m('span.fm-basket-count', t('basket', 'count', basket.items.length)),
          m(
            'button.fm-basket-tool.fm-basket-add',
            { type: 'button', onclick: attrs.onAdd, disabled: attrs.busy },
            `${attrs.addShortcut ? `${attrs.addShortcut} ` : ''}${t('basket', 'addShort')}`,
          ),
        ]),
        m('.fm-basket-columns', [
          m('span.fm-basket-column-name', t('table', 'name')),
          m('span', t('basket', 'location')),
          m('span', { 'aria-hidden': 'true' }),
        ]),
        m(
          '.fm-basket-list',
          {
            role: 'list',
            'aria-label': t('basket', 'list'),
            'aria-description': t('basket', 'hint'),
            title: t('basket', 'hint'),
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
                      class: (start + offset) % 2 === 1 ? 'fm-basket-row-striped' : '',
                      role: 'listitem',
                      style: { top: `${(start + offset) * ROW_HEIGHT}px` },
                    },
                    [
                      m('label.fm-basket-select', [
                        m('input.fm-basket-checkbox', {
                          type: 'checkbox',
                          checked: selected.has(item.key),
                          'aria-label': t('basket', 'select', { name: item.name }),
                          onchange: () => attrs.onToggle(item.key),
                        }),
                        m('span'),
                      ]),
                      m('.fm-basket-entry', [
                        m('span.fm-basket-name', { title: item.name }, item.name),
                        item.status === 'ready'
                          ? undefined
                          : m(
                              'span.fm-basket-warning',
                              { role: 'status' },
                              item.status === 'missing'
                                ? t('basket', 'missing')
                                : item.status === 'moved'
                                  ? t('basket', 'moved')
                                  : t('basket', 'stale'),
                            ),
                      ]),
                      m(
                        'span.fm-basket-location',
                        { title: `${item.location.providerId} · ${item.location.uri}` },
                        item.location.uri,
                      ),
                      m(
                        'button.fm-basket-remove',
                        {
                          type: 'button',
                          'aria-label': t('basket', 'remove', { name: item.name }),
                          onclick: () => attrs.onRemove(item.key),
                        },
                        '×',
                      ),
                    ],
                  ),
                ),
              ),
        ),
        m('.fm-basket-actions', [
          m('.fm-basket-options', [
            m(
              'button.fm-basket-tool',
              { type: 'button', onclick: attrs.onRefresh, disabled: attrs.busy },
              t('basket', 'check'),
            ),
            m(
              'button.fm-basket-tool',
              { type: 'button', onclick: attrs.onClear, disabled: basket.items.length === 0 },
              t('basket', 'clear'),
            ),
            m('label.fm-basket-persist', [
              m('input.fm-basket-checkbox', {
                type: 'checkbox',
                checked: basket.persist,
                onchange: (event: Event) =>
                  attrs.onPersist((event.currentTarget as HTMLInputElement).checked),
              }),
              m('span.fm-basket-persist-label', t('basket', 'persist')),
            ]),
          ]),
          attrs.destination === undefined
            ? m('span.fm-basket-destination', t('basket', 'noDestination'))
            : m(
                'span.fm-basket-destination',
                { title: attrs.destination.uri },
                t('basket', 'destination', { uri: attrs.destination.uri }),
              ),
          ...(['copy', 'move', 'checksum', 'archive', 'delete'] as const).map((kind) =>
            m(
              'button.fm-basket-action',
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
        m('.fm-pane-status.fm-basket-status', { role: 'status' }, [
          m(
            'span',
            formatListingSummary(summary.fileCount, summary.folderCount, summary.knownSize),
          ),
          summary.selectedCount === 0
            ? undefined
            : m(
                'span',
                t('pane', 'selectedSummary', {
                  size: sizeLabel(summary.selectedKnownSize),
                  count: summary.selectedCount,
                }),
              ),
          summary.incompleteSize ? m('span', t('basket', 'knownSizesOnly')) : undefined,
          summary.unavailableCount === 0
            ? undefined
            : m('span', t('basket', 'unavailableCount', summary.unavailableCount)),
        ]),
      ]);
    },
  };
};
