import m, { type FactoryComponent } from 'mithril';
import type { FileManagerClient } from '../../api/client/file-manager-client';
import { tooltip } from '../../components/tooltip';
import { geoMessageText, t } from '../../i18n';
import type {
  CsvGeoOptions,
  GeoDocument,
  GeoProfilePoint,
  GeoTrack,
  MapSettings,
} from '../../models';
import { resolvedTheme } from '../../utilities/resolved-theme';
import { DEFAULT_MAP_SETTINGS } from '../map/map-settings';
import { MapView } from '../map/map-view';
import { SidePanelResizer, sidePanelStyle } from '../map/side-panel-width';
import { sanitizePluginPreviewHtml } from '../preview/plugin-preview';
import { CsvGeoDialog } from './csv-geo-dialog';
import { ElevationProfile } from './elevation-profile';
import { describeValue, featureLabel } from './geo-feature-model';
import { GeoFeaturePanel } from './geo-feature-panel';
import { layersOf, visibleFeatures } from './geo-layers';
import { formatDistance, formatDuration, formatElevation, formatSpeed } from './track-format';

export interface GeoViewAttrs {
  readonly client: Pick<FileManagerClient, 'mapTileUrlTemplate' | 'mediaUrl' | 'getSettings'>;
  readonly name: string;
  readonly document: GeoDocument;
  readonly selectedIndex: number | undefined;
  readonly onSelect: (index: number | undefined) => void;
  readonly onShowRaw: () => void;
  /** Re-reads a CSV file with the chosen columns and coordinate system. */
  readonly onCsvOptions?: (options: CsvGeoOptions) => void;
}

function describeError(document: GeoDocument): m.Children {
  const error = document.error;
  if (error == null) return undefined;
  return m('.fm-geo-error', { role: 'alert' }, [
    m('strong', t('geo', 'errorTitle')),
    m('p', geoMessageText(error)),
    error.line != null && error.column != null
      ? m('p', t('geo', 'errorAt', { line: error.line, column: error.column }))
      : undefined,
    error.path != null && error.path !== ''
      ? m('p', t('geo', 'errorPath', { path: error.path }))
      : undefined,
  ]);
}

interface Stat {
  readonly label: string;
  readonly value: string;
}

function stat(label: string, value: string | undefined): Stat | undefined {
  return value === undefined ? undefined : { label, value };
}

/** Two label/value pairs on one line; when only one exists the other half stays empty. */
function statPair(left: Stat | undefined, right: Stat | undefined): m.Children {
  if (left === undefined && right === undefined) return undefined;
  const half = (item: Stat | undefined) =>
    item === undefined ? [m('dt'), m('dd')] : [m('dt', item.label), m('dd', item.value)];
  return [half(left), half(right)];
}

/** A value too long to share its line (timestamps) takes the whole width. */
function statWide(item: Stat | undefined): m.Children {
  return item === undefined ? undefined : [m('dt', item.label), m('dd.is-wide', item.value)];
}

/** Distance, durations, climb and speeds of a recorded track; unavailable values are not shown. */
function trackSummary(track: GeoTrack, onHover: (point: GeoProfilePoint | undefined) => void) {
  const present = (value: number | null | undefined): value is number =>
    value !== null && value !== undefined;
  const hasTimes = present(track.elapsedSeconds);
  const hasElevation = present(track.minElevation);
  return m('.fm-track-summary', { key: track.featureIndex }, [
    m('h5', track.name ?? t('geoTrack', 'unnamed')),
    m('dl', [
      statPair(
        stat(t('geoTrack', 'distance'), formatDistance(track.distanceMeters)),
        stat(t('geoTrack', 'points'), String(track.points)),
      ),
      statPair(
        stat(
          t('geoTrack', 'ascent'),
          present(track.ascentMeters) ? formatElevation(track.ascentMeters) : undefined,
        ),
        stat(
          t('geoTrack', 'descent'),
          present(track.descentMeters) ? formatElevation(track.descentMeters) : undefined,
        ),
      ),
      statPair(
        stat(
          t('geoTrack', 'minElevation'),
          present(track.minElevation) ? formatElevation(track.minElevation) : undefined,
        ),
        stat(
          t('geoTrack', 'maxElevation'),
          present(track.maxElevation) ? formatElevation(track.maxElevation) : undefined,
        ),
      ),
      statPair(
        stat(
          t('geoTrack', 'elapsed'),
          present(track.elapsedSeconds) ? formatDuration(track.elapsedSeconds) : undefined,
        ),
        stat(
          t('geoTrack', 'moving'),
          present(track.movingSeconds) ? formatDuration(track.movingSeconds) : undefined,
        ),
      ),
      statPair(
        stat(
          t('geoTrack', 'averageSpeed'),
          present(track.averageSpeed) ? formatSpeed(track.averageSpeed) : undefined,
        ),
        stat(
          t('geoTrack', 'maxSpeed'),
          present(track.maxSpeed) ? formatSpeed(track.maxSpeed) : undefined,
        ),
      ),
      statWide(stat(t('geoTrack', 'start'), track.startTime ?? undefined)),
      statWide(stat(t('geoTrack', 'end'), track.endTime ?? undefined)),
    ]),
    hasTimes ? undefined : m('p.fm-geo-hint', t('geoTrack', 'noTimes')),
    hasElevation ? undefined : m('p.fm-geo-hint', t('geoTrack', 'noElevation')),
    m(ElevationProfile, {
      name: track.name ?? t('geoTrack', 'unnamed'),
      points: track.profile,
      onHover,
    }),
  ]);
}

/** Name and sanitised description of a feature, for the popup (links and images are inert). */
function describeFeature(
  document: GeoDocument,
  index: number | undefined,
): HTMLElement | undefined {
  const properties =
    index === undefined ? undefined : document.collection.features[index]?.properties;
  const name = typeof properties?.name === 'string' ? properties.name : undefined;
  const description =
    typeof properties?.description === 'string' ? properties.description : undefined;
  if (name === undefined && description === undefined) return undefined;
  const root = window.document.createElement('div');
  root.className = 'fm-geo-popup';
  if (name !== undefined) {
    const title = window.document.createElement('strong');
    title.textContent = name;
    root.append(title);
  }
  if (description !== undefined) {
    const body = window.document.createElement('div');
    body.innerHTML = sanitizePluginPreviewHtml(description);
    root.append(body);
  }
  return root;
}

const HOVER_MAX_ROWS = 12;
const HOVER_MAX_TEXT = 80;

/** Key/value tooltip of a feature's properties, for hovering the map while the list is collapsed. */
function describeProperties(
  document: GeoDocument,
  index: number | undefined,
): HTMLElement | undefined {
  const feature = index === undefined ? undefined : document.collection.features[index];
  if (feature === undefined) return undefined;
  const entries = Object.entries(feature.properties ?? {});
  const root = window.document.createElement('div');
  root.className = 'fm-geo-hover-content';
  const label = featureLabel(feature);
  if (label !== '') {
    const title = window.document.createElement('strong');
    title.textContent = label;
    root.append(title);
  }
  const list = window.document.createElement('dl');
  for (const [key, value] of entries.slice(0, HOVER_MAX_ROWS)) {
    const term = window.document.createElement('dt');
    term.textContent = key;
    const text = describeValue(value).text;
    const detail = window.document.createElement('dd');
    detail.textContent = text.length > HOVER_MAX_TEXT ? `${text.slice(0, HOVER_MAX_TEXT)}…` : text;
    list.append(term, detail);
  }
  if (entries.length === 0) {
    const none = window.document.createElement('dd');
    none.textContent = t('geo', 'noProperties');
    list.append(none);
  } else if (entries.length > HOVER_MAX_ROWS) {
    const more = window.document.createElement('dd');
    more.textContent = `+${entries.length - HOVER_MAX_ROWS}`;
    list.append(more);
  }
  root.append(list);
  return root;
}

/**
 * The map settings fetched by the last viewer. A viewer that opens later starts from them, rather
 * than from the built-in raster default - which, when a vector style is configured, makes the
 * first tile requests fail until the real settings arrive.
 */
let lastLoadedMapSettings: MapSettings | undefined;

/** How a CSV file was read, with the way to change it. */
function csvSummary(document: GeoDocument, onChange: (() => void) | undefined): m.Children {
  const csv = document.csv;
  if (csv == null) return undefined;
  const crs = `${csv.crs ?? ''}${csv.crsName ? ` (${csv.crsName})` : ''}${
    csv.crsGuessed ? ` ${t('geoCsv', 'guessed')}` : ''
  }`;
  const columns =
    csv.wktColumn != null
      ? t('geoCsv', 'summaryWkt', { column: csv.wktColumn, crs })
      : t('geoCsv', 'summary', {
          columns: [csv.xColumn, csv.yColumn].filter(Boolean).join(' / '),
          crs,
        });
  return m('.fm-geo-csv', [
    m('p', columns),
    csv.skippedRows > 0
      ? m('p.fm-geo-warning', t('geoCsv', 'rowsSkipped', { n: csv.skippedRows }))
      : undefined,
    onChange === undefined
      ? undefined
      : m('button.fm-geo-csv-change', { type: 'button', onclick: onChange }, t('geoCsv', 'change')),
  ]);
}

type PanelMode = 'split' | 'full' | 'collapsed';

function panelButton(label: string, glyph: string, onclick: () => void): m.Vnode {
  return tooltip(
    label,
    m('button.fm-geo-panel-button', { type: 'button', 'aria-label': label, onclick }, glyph),
  );
}

/**
 * GeoJSON on the shared map (task 0277): features drawn with simplestyle colours, click for the
 * property table, and the parse warnings, limits and skipped elements stated beside the map.
 */
export const GeoView: FactoryComponent<GeoViewAttrs> = (initial) => {
  let mapSettings: MapSettings = lastLoadedMapSettings ?? DEFAULT_MAP_SETTINGS;
  let requested = false;
  // Transient view state: switched-off KML folders.
  let hiddenLayers: ReadonlySet<string> = new Set();
  let visibleKey: GeoDocument | undefined;
  let visibleSet: ReadonlySet<string> | undefined;
  let visible = visibleFeatures(initial.attrs.document, hiddenLayers);
  // Transient: the chart sample under the pointer, shown as a marker on the map.
  let cursor: GeoProfilePoint | undefined;
  // Transient: how much of the pane the feature list takes.
  let mode: PanelMode = 'split';
  let csvDialogOpen = false;
  return {
    oncreate: ({ attrs }) => {
      if (requested) return;
      requested = true;
      void attrs.client
        .getSettings()
        .then((settings) => {
          mapSettings = settings.map ?? DEFAULT_MAP_SETTINGS;
          lastLoadedMapSettings = mapSettings;
          m.redraw();
        })
        .catch(() => undefined);
    },
    view: ({ attrs }) => {
      const { document } = attrs;
      const csvDialog = (): m.Children =>
        document.csv == null || attrs.onCsvOptions === undefined
          ? undefined
          : m(CsvGeoDialog, {
              open: csvDialogOpen,
              info: document.csv,
              onApply: (options) => {
                csvDialogOpen = false;
                attrs.onCsvOptions?.(options);
              },
              onCancel: () => {
                csvDialogOpen = false;
              },
            });
      if (document.error != null) {
        return m('.fm-geo-view.is-error', [
          describeError(document),
          document.csv == null || attrs.onCsvOptions === undefined
            ? undefined
            : m(
                'button.fm-geo-csv-change',
                { type: 'button', onclick: () => (csvDialogOpen = true) },
                t('geoCsv', 'change'),
              ),
          m('button.btn-flat', { type: 'button', onclick: attrs.onShowRaw }, t('geo', 'showRaw')),
          csvDialog(),
        ]);
      }
      if (visibleKey !== document || visibleSet !== hiddenLayers) {
        visible = visibleFeatures(document, hiddenLayers);
        visibleKey = document;
        visibleSet = hiddenLayers;
      }
      const shownIndex =
        attrs.selectedIndex === undefined
          ? undefined
          : visible.originalIndexes.indexOf(attrs.selectedIndex);
      const layers = layersOf(document);
      const extra: m.Children = [
        csvSummary(
          document,
          attrs.onCsvOptions === undefined ? undefined : () => (csvDialogOpen = true),
        ),
        layers.length === 0
          ? undefined
          : m('.fm-geo-layers', [
              m('h5', t('geoLayers', 'title')),
              m(
                'ul',
                layers.map((layer) =>
                  m('li', { key: layer.path, style: { paddingLeft: `${layer.depth}rem` } }, [
                    m('label', [
                      m('input', {
                        type: 'checkbox',
                        checked: !hiddenLayers.has(layer.path),
                        'aria-label': t('geoLayers', 'show', { name: layer.name }),
                        onchange: (event: Event) => {
                          const next = new Set(hiddenLayers);
                          if ((event.currentTarget as HTMLInputElement).checked) {
                            next.delete(layer.path);
                          } else next.add(layer.path);
                          hiddenLayers = next;
                        },
                      }),
                      m('span', ` ${layer.name} (${layer.count})`),
                    ]),
                  ]),
                ),
              ),
            ]),
        document.truncated
          ? m(
              'p.fm-geo-warning',
              t('geo', 'truncated', {
                shown: document.displayedFeatures,
                total: document.totalFeatures,
              }),
            )
          : undefined,
        document.warnings.length === 0
          ? undefined
          : m('.fm-geo-warnings', [
              m('h5', t('geo', 'warnings')),
              m(
                'ul',
                document.warnings.map((warning, index) =>
                  m('li', { key: index }, [
                    warning.path === '' ? undefined : m('code', `${warning.path} `),
                    geoMessageText(warning),
                  ]),
                ),
              ),
            ]),
        document.skipped.length === 0
          ? undefined
          : m('.fm-geo-skipped', [
              m('h5', t('geo', 'skipped')),
              m(
                'ul',
                document.skipped.map((item, index) => m('li', { key: index }, item)),
              ),
            ]),
        (document.tracks ?? []).map((track) =>
          trackSummary(track, (point) => {
            cursor = point;
            m.redraw();
          }),
        ),
      ];
      return m('.fm-geo-view', { class: `is-${mode}` }, [
        csvDialog(),
        m('aside.fm-geo-side', mode === 'split' ? { style: sidePanelStyle() } : {}, [
          mode === 'collapsed'
            ? m(
                '.fm-geo-side-head',
                panelButton(t('geo', 'showList'), '»', () => (mode = 'split')),
              )
            : [
                m('.fm-geo-side-head', [
                  m('span.fm-geo-summary', t('geo', 'summary', document.totalFeatures)),
                  panelButton(
                    mode === 'full' ? t('geo', 'restoreList') : t('geo', 'maximizeList'),
                    mode === 'full' ? '⤡' : '⤢',
                    () => (mode = mode === 'full' ? 'split' : 'full'),
                  ),
                  panelButton(t('geo', 'collapseList'), '«', () => (mode = 'collapsed')),
                ]),
                m(GeoFeaturePanel, {
                  document,
                  originalIndexes: visible.originalIndexes,
                  selectedIndex: attrs.selectedIndex,
                  onSelect: attrs.onSelect,
                }),
                m('.fm-geo-extra', extra),
              ],
        ]),
        mode === 'split' ? m(SidePanelResizer) : undefined,
        m(
          '.fm-geo-map',
          m(MapView, {
            client: attrs.client,
            settings: mapSettings,
            dark: resolvedTheme() === 'dark',
            markers: [],
            features: visible.collection,
            icons: document.resources ?? {},
            overlays: (document.overlays ?? []).flatMap((overlay, index) => {
              const url = document.resources?.[overlay.image];
              return url === undefined ? [] : [{ id: String(index), url, ...overlay }];
            }),
            renderFeaturePopup: (index) =>
              describeFeature(document, visible.originalIndexes[index]),
            ...(mode === 'collapsed'
              ? {
                  renderFeatureHover: (index: number) =>
                    describeProperties(document, visible.originalIndexes[index]),
                }
              : {}),
            ...(document.bounds == null ? {} : { dataBounds: document.bounds }),
            ...(shownIndex === undefined || shownIndex < 0 ? {} : { highlightIndex: shownIndex }),
            fitKey: document.displayedFeatures,
            ariaLabel: t('geo', 'mapLabel', { name: attrs.name }),
            onFeatureClick: (index) =>
              attrs.onSelect(index === undefined ? undefined : visible.originalIndexes[index]),
            ...(cursor === undefined
              ? {}
              : { cursor: { latitude: cursor.latitude, longitude: cursor.longitude } }),
          }),
        ),
      ]);
    },
  };
};
