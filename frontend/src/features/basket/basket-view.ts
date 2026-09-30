import m, { type FactoryComponent } from 'mithril';
import { copyIcon, trashIcon } from '../../components/tabler-icons';
import { t } from '../../i18n';
import type { Location } from '../../models';
import { formatListingSummary, sizeLabel } from '../panes/pane-status-summary';
import { type BasketState, basketSummary } from './basket';

const ROW_HEIGHT = 20;

function basketGlyph(path: string): m.Children {
  return m(
    'svg.fm-icon.fm-icon-tabler',
    {
      'aria-hidden': 'true',
      viewBox: '0 0 24 24',
      width: 16,
      height: 16,
      fill: 'none',
      stroke: 'currentColor',
      'stroke-width': 2,
      'stroke-linecap': 'round',
      'stroke-linejoin': 'round',
    },
    m('path', { d: path }),
  );
}

export interface BasketViewAttrs {
  readonly basket: BasketState;
  readonly destination?: Location;
  readonly busy: boolean;
  readonly sizingFolders: boolean;
  readonly addShortcut?: string;
  readonly onOpen: () => void;
  readonly onClose: () => void;
  readonly onAdd: () => void;
  readonly onToggle: (key: string) => void;
  readonly onSelectAll: () => void;
  readonly onDeselectAll: () => void;
  readonly onRemove: (key: string) => void;
  readonly onClear: () => void;
  readonly onAction: (kind: 'copy' | 'move' | 'checksum' | 'archive' | 'delete') => void;
}

/** Virtualized collection list; the native checkboxes/buttons provide keyboard and screen-reader access. */
export const BasketView: FactoryComponent<BasketViewAttrs> = () => {
  let scrollTop = 0;
  let viewportHeight = 600;
  let observer: ResizeObserver | undefined;
  let nameWidth: number | undefined;
  let stopResize: (() => void) | undefined;
  return {
    oncreate: ({ attrs }) => attrs.onOpen(),
    onremove: ({ attrs }) => {
      observer?.disconnect();
      stopResize?.();
      attrs.onClose();
    },
    view: ({ attrs }) => {
      const { basket } = attrs;
      const start = Math.max(0, Math.floor(scrollTop / ROW_HEIGHT) - 4);
      const end = Math.min(basket.items.length, start + Math.ceil(viewportHeight / ROW_HEIGHT) + 8);
      const selected = new Set(basket.selectedKeys);
      const summary = basketSummary(basket);
      const actionable = basket.items.some(
        (item) => item.status === 'ready' && selected.has(item.key),
      );
      const actionableFile = basket.items.some(
        (item) => item.kind === 'file' && item.status === 'ready' && selected.has(item.key),
      );
      const gridStyle =
        nameWidth === undefined ? undefined : { '--fm-basket-name-width': `${nameWidth}px` };
      const iconButton = (
        label: string,
        icon: m.Children,
        onclick: () => void,
        disabled: boolean,
        className: string,
      ): m.Children =>
        m(
          `button.btn-flat.${className}`,
          { type: 'button', title: label, 'aria-label': label, onclick, disabled },
          icon,
        );
      return m('.fm-basket', [
        m('.fm-basket-header', [
          m('span.fm-basket-count', t('basket', 'count', basket.items.length)),
          m(
            'button.fm-basket-tool.fm-basket-add',
            { type: 'button', onclick: attrs.onAdd, disabled: attrs.busy },
            `${attrs.addShortcut ? `${attrs.addShortcut} ` : ''}${t('basket', 'addShort')}`,
          ),
        ]),
        m('.fm-basket-columns', { style: gridStyle }, [
          m('span.fm-basket-column-name', [
            t('table', 'name'),
            m('span.fm-basket-resize-handle', {
              role: 'separator',
              tabindex: 0,
              'aria-label': t('basket', 'resizeName'),
              'aria-orientation': 'vertical',
              onpointerdown: (event: PointerEvent) => {
                event.preventDefault();
                stopResize?.();
                const handle = event.currentTarget as HTMLElement;
                handle.setPointerCapture?.(event.pointerId);
                const width = handle.parentElement?.getBoundingClientRect().width ?? 160;
                const startX = event.clientX;
                const maxWidth = Math.max(
                  80,
                  (handle.closest('.fm-basket')?.clientWidth ?? 400) - 150,
                );
                const move = (moveEvent: PointerEvent) => {
                  nameWidth = Math.max(
                    80,
                    Math.min(maxWidth, Math.round(width - 24 + moveEvent.clientX - startX)),
                  );
                  m.redraw();
                };
                const end = () => stopResize?.();
                stopResize = () => {
                  window.removeEventListener('pointermove', move);
                  window.removeEventListener('pointerup', end);
                  stopResize = undefined;
                };
                window.addEventListener('pointermove', move);
                window.addEventListener('pointerup', end);
              },
              onkeydown: (event: KeyboardEvent) => {
                if (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight') return;
                event.preventDefault();
                const width =
                  (event.currentTarget as HTMLElement).parentElement?.getBoundingClientRect()
                    .width ?? 160;
                const maxWidth = Math.max(
                  80,
                  ((event.currentTarget as HTMLElement).closest('.fm-basket')?.clientWidth ?? 400) -
                    150,
                );
                nameWidth = Math.max(
                  80,
                  Math.min(
                    maxWidth,
                    Math.round(width - 24 + (event.key === 'ArrowRight' ? 10 : -10)),
                  ),
                );
                m.redraw();
              },
            }),
          ]),
          m('span', t('basket', 'location')),
          m('span', { 'aria-hidden': 'true' }),
        ]),
        m(
          '.fm-basket-list',
          {
            role: 'list',
            'aria-label': t('basket', 'list'),
            'aria-description': t('basket', 'hint'),
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
                      style: {
                        ...gridStyle,
                        top: `${(start + offset) * ROW_HEIGHT}px`,
                      },
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
            iconButton(
              t('basket', 'selectAll'),
              basketGlyph('M4 4h16v16H4z M7 12l3 3 6-6'),
              attrs.onSelectAll,
              attrs.busy || basket.items.length === 0 || selected.size === basket.items.length,
              'fm-basket-tool',
            ),
            iconButton(
              t('basket', 'deselectAll'),
              basketGlyph('M4 4h16v16H4z M8 12h8'),
              attrs.onDeselectAll,
              attrs.busy || selected.size === 0,
              'fm-basket-tool',
            ),
            iconButton(
              t('basket', 'clear'),
              basketGlyph('M3 10h18l-2 11H5L3 10z M7 10l5-7 5 7 M9 14l6 4m0-4-6 4'),
              attrs.onClear,
              attrs.busy || basket.items.length === 0,
              'fm-basket-tool fm-basket-clear',
            ),
          ]),
          m(
            '.fm-basket-operations',
            (['copy', 'move', 'checksum', 'archive', 'delete'] as const).map((kind) =>
              iconButton(
                kind === 'checksum'
                  ? t('checksums', 'title')
                  : kind === 'archive'
                    ? t('archiveCreate', 'createTitle')
                    : kind === 'delete'
                      ? t('button', 'delete')
                      : t('operation', kind),
                kind === 'copy'
                  ? copyIcon({ size: 16 })
                  : kind === 'delete'
                    ? trashIcon({ size: 16 })
                    : kind === 'move'
                      ? basketGlyph('M4 6h7v4 M4 6v13h16v-8 M9 12h11m-4-4 4 4-4 4')
                      : kind === 'checksum'
                        ? basketGlyph('M12 3l8 3v6c0 5-3 8-8 9-5-1-8-4-8-9V6z M9 10h6m-6 4h6')
                        : basketGlyph('M3 5h18v4H3z M5 9v11h14V9 M12 11v6m-3-3 3 3 3-3'),
                () => attrs.onAction(kind),
                attrs.busy ||
                  (kind === 'checksum' ? !actionableFile : !actionable) ||
                  ((kind === 'copy' || kind === 'move' || kind === 'archive') &&
                    attrs.destination === undefined),
                'fm-basket-action',
              ),
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
          attrs.sizingFolders
            ? m('span', t('basket', 'calculatingSizes'))
            : summary.incompleteSize
              ? m('span', t('basket', 'knownSizesOnly'))
              : undefined,
          summary.unavailableCount === 0
            ? undefined
            : m('span', t('basket', 'unavailableCount', summary.unavailableCount)),
        ]),
      ]);
    },
  };
};
