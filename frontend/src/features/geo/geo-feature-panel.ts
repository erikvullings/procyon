import m, { type FactoryComponent } from 'mithril';
import { tooltip } from '../../components/tooltip';
import { t } from '../../i18n';
import type { GeoDocument } from '../../models';
import {
  featureLabel,
  type GeometryKind,
  geometryKind,
  type PropertyRow,
  propertyRows,
} from './geo-feature-model';

export interface GeoFeaturePanelAttrs {
  readonly document: GeoDocument;
  /** Positions in the document of the features that are shown, in list order. */
  readonly originalIndexes: readonly number[];
  readonly selectedIndex: number | undefined;
  readonly onSelect: (index: number | undefined) => void;
}

type ListRow =
  | { readonly kind: 'feature'; readonly original: number }
  | { readonly kind: 'property'; readonly row: PropertyRow }
  | { readonly kind: 'empty' };

const FALLBACK_ROW_HEIGHT = 20;
const OVERSCAN = 8;
const ICON_SIZE = 14;

const SHAPES: Record<GeometryKind, readonly string[]> = {
  point: ['M12 7a5 5 0 1 0 0 10a5 5 0 0 0 0-10Z'],
  multipoint: ['M7 6a3 3 0 1 0 0 6a3 3 0 0 0 0-6Z', 'M17 12a3 3 0 1 0 0 6a3 3 0 0 0 0-6Z'],
  line: ['M3 19L9 7l6 8l6-11'],
  multiline: ['M3 9L9 4l6 5l6-5', 'M3 20l6-5l6 5l6-5'],
  polygon: ['M12 3l9 7l-3.5 11h-11L3 10Z'],
  multipolygon: ['M8 3l6 5l-2.5 8h-7L2 8Z', 'M17 11l5 4l-2 6h-6l-2-6Z'],
  collection: ['M4 4h7v7H4Z', 'M13 13h7v7h-7Z'],
  none: ['M5 12h14'],
};

const FILLED: ReadonlySet<GeometryKind> = new Set(['point', 'multipoint']);

function kindLabel(kind: GeometryKind, geometryType: string | undefined): string {
  return kind === 'none' ? t('geo', 'noGeometry') : (geometryType ?? kind);
}

/** A small pictogram of the geometry type: dot(s), line(s), polygon(s). */
function geometryIcon(kind: GeometryKind): m.Vnode {
  const filled = FILLED.has(kind);
  return m(
    'svg.fm-geo-kind',
    { 'aria-hidden': 'true', viewBox: '0 0 24 24', width: ICON_SIZE, height: ICON_SIZE },
    SHAPES[kind].map((d) =>
      m('path', {
        d,
        fill: filled ? 'currentColor' : 'none',
        stroke: 'currentColor',
        'stroke-width': 2,
        'stroke-linejoin': 'round',
        'stroke-linecap': 'round',
      }),
    ),
  );
}

function cell(text: string, className: string): m.Vnode {
  return m(`.fm-geo-cell.${className}`, tooltip(text, m('span.fm-geo-cell-text', text)));
}

function propertyRow(row: PropertyRow, toggle: (path: string) => void): m.Vnode {
  const { view } = row;
  const label = view.nested ? (row.expanded ? '▾' : '▸') : '';
  return m(
    '.fm-geo-row.fm-geo-property',
    {
      key: row.path,
      class: view.nested ? 'is-nested' : undefined,
      role: 'row',
      onclick: view.nested ? () => toggle(row.path) : undefined,
    },
    [
      m('.fm-geo-key', { style: { paddingInlineStart: `${row.depth}rem` } }, [
        m('span.fm-geo-twisty', label),
        cell(row.key, 'is-key'),
      ]),
      cell(view.text, `is-value.is-${view.type}`),
    ],
  );
}

/**
 * The features of a document as a virtualised list, one fixed-height row each, in the pane-like
 * side panel of the map. The selected feature unfolds its properties beneath it as key/value
 * rows; nested objects unfold as a tree.
 */
export const GeoFeaturePanel: FactoryComponent<GeoFeaturePanelAttrs> = () => {
  let scrollTop = 0;
  let viewport = 0;
  let rowHeight = FALLBACK_ROW_HEIGHT;
  let expanded: ReadonlySet<string> = new Set();
  let expandedFor: number | undefined;
  let revealed: number | undefined;
  let scroller: HTMLElement | undefined;

  const measure = (): void => {
    if (scroller === undefined) return;
    const height = parseFloat(getComputedStyle(scroller).getPropertyValue('--fm-row-height'));
    const next = Number.isFinite(height) && height > 0 ? height : FALLBACK_ROW_HEIGHT;
    if (next !== rowHeight || scroller.clientHeight !== viewport) {
      rowHeight = next;
      viewport = scroller.clientHeight;
      m.redraw();
    }
  };

  const toggle = (path: string): void => {
    const next = new Set(expanded);
    if (!next.delete(path)) next.add(path);
    expanded = next;
  };

  const buildRows = (attrs: GeoFeaturePanelAttrs): ListRow[] => {
    const { document, originalIndexes, selectedIndex } = attrs;
    if (selectedIndex !== expandedFor) {
      expandedFor = selectedIndex;
      expanded = new Set();
    }
    const rows: ListRow[] = [];
    for (const original of originalIndexes) {
      rows.push({ kind: 'feature', original });
      if (original !== selectedIndex) continue;
      const properties = document.collection.features[original]?.properties;
      const flat = propertyRows(properties, expanded);
      if (flat.length === 0) rows.push({ kind: 'empty' });
      for (const row of flat) rows.push({ kind: 'property', row });
    }
    return rows;
  };

  /** Scrolls the selected feature into view when it is not fully visible (centred otherwise). */
  const reveal = (rows: readonly ListRow[], selectedIndex: number | undefined): void => {
    if (selectedIndex === revealed) return;
    revealed = selectedIndex;
    if (scroller === undefined || selectedIndex === undefined) return;
    const at = rows.findIndex((row) => row.kind === 'feature' && row.original === selectedIndex);
    if (at < 0) return;
    const top = at * rowHeight;
    if (top < scroller.scrollTop || top + rowHeight > scroller.scrollTop + scroller.clientHeight) {
      scroller.scrollTop = Math.max(0, top - (scroller.clientHeight - rowHeight) / 2);
    }
  };

  return {
    oncreate: ({ dom }) => {
      scroller = dom.querySelector<HTMLElement>('.fm-geo-scroller') ?? undefined;
      measure();
    },
    onupdate: ({ attrs }) => {
      measure();
      reveal(buildRows(attrs), attrs.selectedIndex);
    },
    view: ({ attrs }) => {
      const { document, selectedIndex, onSelect } = attrs;
      const rows = buildRows(attrs);
      const height = viewport > 0 ? viewport : rowHeight * 20;
      const first = Math.max(0, Math.floor(scrollTop / rowHeight) - OVERSCAN);
      const last = Math.min(rows.length, Math.ceil((scrollTop + height) / rowHeight) + OVERSCAN);
      return m('.fm-geo-feature-panel', [
        m(
          '.fm-geo-scroller',
          {
            role: 'treegrid',
            'aria-label': t('geo', 'featureList'),
            onscroll: (event: Event) => {
              scrollTop = (event.currentTarget as HTMLElement).scrollTop;
            },
          },
          m('.fm-geo-rows', { style: { height: `${rows.length * rowHeight}px` } }, [
            m(
              '.fm-geo-window',
              { style: { transform: `translateY(${first * rowHeight}px)` } },
              rows.slice(first, last).map((row) => {
                if (row.kind === 'property') return propertyRow(row.row, toggle);
                if (row.kind === 'empty') {
                  return m('.fm-geo-row.fm-geo-empty', { key: 'empty' }, t('geo', 'noProperties'));
                }
                const feature = document.collection.features[row.original];
                if (feature === undefined) return undefined;
                const kind = geometryKind(feature.geometry);
                const selected = row.original === selectedIndex;
                const label = featureLabel(feature);
                const typeLabel = kindLabel(kind, feature.geometry?.type);
                return m(
                  'button.fm-geo-row.fm-geo-feature',
                  {
                    key: `f${row.original}`,
                    type: 'button',
                    role: 'row',
                    class: selected ? 'is-selected' : undefined,
                    'aria-expanded': String(selected),
                    onclick: () => onSelect(selected ? undefined : row.original),
                  },
                  [
                    tooltip(typeLabel, geometryIcon(kind), { class: 'fm-geo-kind-wrap' }),
                    label === '' ? m('span.fm-geo-label.is-empty') : cell(label, 'is-label'),
                  ],
                );
              }),
            ),
          ]),
        ),
      ]);
    },
  };
};
