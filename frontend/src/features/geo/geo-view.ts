import m, { type FactoryComponent } from 'mithril';
import type { FileManagerClient } from '../../api/client/file-manager-client';
import { t } from '../../i18n';
import type { GeoDocument, MapSettings } from '../../models';
import { resolvedTheme } from '../../utilities/resolved-theme';
import { DEFAULT_MAP_SETTINGS } from '../map/map-settings';
import { MapView } from '../map/map-view';
import { propertyText } from './geo-document';

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
              m('tr', { key }, [m('th', { scope: 'row' }, key), m('td', propertyText(value))]),
            ),
          ),
        ),
  ]);
}

/**
 * GeoJSON on the shared map (task 0277): features drawn with simplestyle colours, click for the
 * property table, and the parse warnings, limits and skipped elements stated beside the map.
 */
export const GeoView: FactoryComponent<GeoViewAttrs> = () => {
  let mapSettings: MapSettings = DEFAULT_MAP_SETTINGS;
  let requested = false;
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
      return m('.fm-geo-view', [
        m(
          '.fm-geo-map',
          m(MapView, {
            client: attrs.client,
            settings: mapSettings,
            dark: resolvedTheme() === 'dark',
            markers: [],
            features: document.collection,
            ...(document.bounds == null ? {} : { dataBounds: document.bounds }),
            ...(attrs.selectedIndex === undefined ? {} : { highlightIndex: attrs.selectedIndex }),
            fitKey: document.displayedFeatures,
            ariaLabel: t('geo', 'mapLabel', { name: attrs.name }),
            onFeatureClick: attrs.onSelect,
          }),
        ),
        m('aside.fm-geo-side', [
          m('p', t('geo', 'summary', { n: document.totalFeatures })),
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
          propertiesTable(document, attrs.selectedIndex),
        ]),
      ]);
    },
  };
};
