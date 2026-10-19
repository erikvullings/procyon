import m, { type FactoryComponent } from 'mithril';
import type { FileManagerClient } from '../../api/client/file-manager-client';
import { t } from '../../i18n';
import type { GeoDocument, GeoProfilePoint, GeoTrack, MapSettings } from '../../models';
import { resolvedTheme } from '../../utilities/resolved-theme';
import { DEFAULT_MAP_SETTINGS } from '../map/map-settings';
import { MapView } from '../map/map-view';
import { sanitizePluginPreviewHtml } from '../preview/plugin-preview';
import { ElevationProfile } from './elevation-profile';
import { propertyText } from './geo-document';
import { layersOf, visibleFeatures } from './geo-layers';
import { formatDistance, formatDuration, formatElevation, formatSpeed } from './track-format';

export interface GeoViewAttrs {
  readonly client: Pick<FileManagerClient, 'mapTileUrlTemplate' | 'mediaUrl' | 'getSettings'>;
  readonly name: string;
  readonly document: GeoDocument;
  readonly selectedIndex: number | undefined;
  readonly onSelect: (index: number | undefined) => void;
  readonly onShowRaw: () => void;
}

function describeError(document: GeoDocument): m.Children {
  const error = document.error;
  if (error == null) return undefined;
  return m('.fm-geo-error', { role: 'alert' }, [
    m('strong', t('geo', 'errorTitle')),
    m('p', error.message),
    error.line != null && error.column != null
      ? m('p', t('geo', 'errorAt', { line: error.line, column: error.column }))
      : undefined,
    error.path != null && error.path !== ''
      ? m('p', t('geo', 'errorPath', { path: error.path }))
      : undefined,
  ]);
}

function statRow(label: string, value: string | undefined): m.Children {
  return value === undefined ? undefined : [m('dt', label), m('dd', value)];
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
      statRow(t('geoTrack', 'distance'), formatDistance(track.distanceMeters)),
      statRow(
        t('geoTrack', 'elapsed'),
        present(track.elapsedSeconds) ? formatDuration(track.elapsedSeconds) : undefined,
      ),
      statRow(
        t('geoTrack', 'moving'),
        present(track.movingSeconds) ? formatDuration(track.movingSeconds) : undefined,
      ),
      statRow(
        t('geoTrack', 'ascent'),
        present(track.ascentMeters) ? formatElevation(track.ascentMeters) : undefined,
      ),
      statRow(
        t('geoTrack', 'descent'),
        present(track.descentMeters) ? formatElevation(track.descentMeters) : undefined,
      ),
      statRow(
        t('geoTrack', 'minElevation'),
        present(track.minElevation) ? formatElevation(track.minElevation) : undefined,
      ),
      statRow(
        t('geoTrack', 'maxElevation'),
        present(track.maxElevation) ? formatElevation(track.maxElevation) : undefined,
      ),
      statRow(
        t('geoTrack', 'averageSpeed'),
        present(track.averageSpeed) ? formatSpeed(track.averageSpeed) : undefined,
      ),
      statRow(
        t('geoTrack', 'maxSpeed'),
        present(track.maxSpeed) ? formatSpeed(track.maxSpeed) : undefined,
      ),
      statRow(t('geoTrack', 'start'), track.startTime ?? undefined),
      statRow(t('geoTrack', 'end'), track.endTime ?? undefined),
      statRow(t('geoTrack', 'points'), String(track.points)),
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

/** Features listed beside the map; the rest stays reachable by clicking on the map. */
const FEATURE_LIST_LIMIT = 300;

function featureTitle(
  feature: GeoDocument['collection']['features'][number],
  index: number,
): string {
  const properties = feature.properties ?? {};
  for (const key of ['name', 'title', 'Name', 'NAME', 'label', 'id']) {
    const value = properties[key];
    if (typeof value === 'string' && value.trim() !== '') return value;
    if (typeof value === 'number') return String(value);
  }
  return t('geo', 'featureUnnamed', { n: index + 1 });
}

function featureList(
  document: GeoDocument,
  originalIndexes: readonly number[],
  selectedIndex: number | undefined,
  onSelect: (index: number | undefined) => void,
): m.Children {
  const features = document.collection.features;
  if (originalIndexes.length === 0) return undefined;
  return m('.fm-geo-feature-list', [
    m('h5', t('geo', 'featureList')),
    m(
      'ul',
      originalIndexes.slice(0, FEATURE_LIST_LIMIT).map((original) => {
        const feature = features[original];
        if (feature === undefined) return undefined;
        const selected = original === selectedIndex;
        return m('li', { key: original }, [
          m(
            'button.fm-geo-feature-item',
            {
              type: 'button',
              class: selected ? 'is-selected' : undefined,
              'aria-pressed': String(selected),
              onclick: () => onSelect(selected ? undefined : original),
            },
            [
              m('span.fm-geo-feature-name', featureTitle(feature, original)),
              m('span.fm-geo-feature-type', feature.geometry?.type ?? t('geo', 'noGeometry')),
            ],
          ),
        ]);
      }),
    ),
    originalIndexes.length > FEATURE_LIST_LIMIT
      ? m('p.fm-geo-hint', t('geo', 'listTruncated', { n: FEATURE_LIST_LIMIT }))
      : undefined,
  ]);
}

function propertiesTable(document: GeoDocument, index: number | undefined): m.Children {
  if (index === undefined) return m('p.fm-geo-hint', t('geo', 'selectFeature'));
  const feature = document.collection.features[index];
  if (feature === undefined) return undefined;
  const entries = Object.entries(feature.properties ?? {});
  return m('.fm-geo-properties', [
    m('h5', t('geo', 'properties')),
    feature.geometry === null
      ? m('p.fm-geo-hint', t('geo', 'noGeometry'))
      : m('p.fm-geo-hint', t('geo', 'legend', { type: feature.geometry.type })),
    entries.length === 0
      ? m('p.fm-geo-hint', t('geo', 'noProperties'))
      : m(
          'table',
          m(
            'tbody',
            entries.map(([key, value]) =>
              m('tr', { key }, [
                m('th', { scope: 'row' }, key),
                m(
                  'td',
                  key === 'description' && typeof value === 'string'
                    ? m.trust(sanitizePluginPreviewHtml(value))
                    : propertyText(value),
                ),
              ]),
            ),
          ),
        ),
  ]);
}

/**
 * GeoJSON on the shared map (task 0277): features drawn with simplestyle colours, click for the
 * property table, and the parse warnings, limits and skipped elements stated beside the map.
 */
export const GeoView: FactoryComponent<GeoViewAttrs> = (initial) => {
  let mapSettings: MapSettings = DEFAULT_MAP_SETTINGS;
  let requested = false;
  // Transient view state: switched-off KML folders.
  let hiddenLayers: ReadonlySet<string> = new Set();
  let visibleKey: GeoDocument | undefined;
  let visibleSet: ReadonlySet<string> | undefined;
  let visible = visibleFeatures(initial.attrs.document, hiddenLayers);
  // Transient: the chart sample under the pointer, shown as a marker on the map.
  let cursor: GeoProfilePoint | undefined;
  return {
    oncreate: ({ attrs }) => {
      if (requested) return;
      requested = true;
      void attrs.client
        .getSettings()
        .then((settings) => {
          mapSettings = settings.map ?? DEFAULT_MAP_SETTINGS;
          m.redraw();
        })
        .catch(() => undefined);
    },
    view: ({ attrs }) => {
      const { document } = attrs;
      if (document.error != null) {
        return m('.fm-geo-view.is-error', [
          describeError(document),
          m('button.btn-flat', { type: 'button', onclick: attrs.onShowRaw }, t('geo', 'showRaw')),
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
      return m('.fm-geo-view', [
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
        m('aside.fm-geo-side', [
          m('p', t('geo', 'summary', document.totalFeatures)),
          featureList(document, visible.originalIndexes, attrs.selectedIndex, attrs.onSelect),
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
                      warning.message,
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
          propertiesTable(document, attrs.selectedIndex),
        ]),
      ]);
    },
  };
};
