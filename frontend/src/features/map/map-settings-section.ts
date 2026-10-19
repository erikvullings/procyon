import m, { type FactoryComponent } from 'mithril';
import { NumberInput, Select, Switch, TextInput } from 'mithril-materialized';
import { t } from '../../i18n';
import type { MapBasemapKind, MapSettings } from '../../models';
import { resolvedTheme } from '../../utilities/resolved-theme';
import {
  DEFAULT_MAP_SETTINGS,
  locationFromPathInput,
  pathInputFromLocation,
  remoteHostOf,
  validateMapSettings,
} from './map-settings';
import { MapView, type MapViewClient } from './map-view';

export interface MapSettingsSectionAttrs {
  /** Needed for the preview map; without it the preview button is hidden. */
  readonly client?: MapViewClient;
  readonly settings: MapSettings;
  readonly onChange: (settings: MapSettings) => void;
}

const KIND_LABELS: Record<MapBasemapKind, 'kindRaster' | 'kindStyle' | 'kindPmtiles'> = {
  raster: 'kindRaster',
  style: 'kindStyle',
  pmtiles: 'kindPmtiles',
};

/** Which host a basemap contacts and who contacts it, so the user knows where requests go. */
export function networkNotice(settings: MapSettings): string {
  if (settings.basemapKind === 'pmtiles') return t('map', 'localNotice');
  if (!settings.onlineTiles) return t('map', 'offlineNotice');
  const host = remoteHostOf(settings);
  if (host === undefined) return '';
  return settings.basemapKind === 'raster'
    ? t('map', 'hostProxied', { host })
    : t('map', 'hostDirect', { host });
}

/** Settings editor section for the basemap of the shared map component (task 0275). */
export const MapSettingsSection: FactoryComponent<MapSettingsSectionAttrs> = () => {
  // Transient: whether the preview map (which loads the map library and tiles) is open.
  let previewOpen = false;
  return {
    view: ({ attrs: { client, settings, onChange } }) => {
      const change = (patch: Partial<MapSettings>): void => onChange({ ...settings, ...patch });
      const problem = validateMapSettings(settings);
      return [
        m('.row', m('h4.fm-settings-section-heading.col.s12', t('map', 'sectionTitle'))),
        m('.row', [
          m(Switch, {
            className: 'col s12',
            label: t('map', 'onlineTiles'),
            checked: settings.onlineTiles,
            left: t('settings', 'off'),
            right: t('settings', 'on'),
            onchange: (onlineTiles: boolean) => change({ onlineTiles }),
          }),
        ]),
        m('.row', [
          m(Select<MapBasemapKind>, {
            className: 'col s12',
            label: t('map', 'basemapKind'),
            options: (['raster', 'style', 'pmtiles'] as const).map((id) => ({
              id,
              label: t('map', KIND_LABELS[id]),
            })),
            checkedId: settings.basemapKind,
            onchange: ([value]) => value !== undefined && change({ basemapKind: value }),
          }),
        ]),
        settings.basemapKind === 'raster'
          ? m('.row', [
              m(TextInput, {
                className: 'col s12',
                label: t('map', 'urlTemplate'),
                helperText: t('map', 'urlTemplateHint'),
                initialValue: settings.urlTemplate,
                oninput: (value: string) => change({ urlTemplate: value }),
              }),
              m(NumberInput, {
                className: 'col s6',
                label: t('map', 'maxZoom'),
                value: settings.maxZoom,
                min: 1,
                max: 24,
                oninput: (value: number) => change({ maxZoom: value }),
              }),
            ])
          : undefined,
        settings.basemapKind === 'style'
          ? m('.row', [
              m(TextInput, {
                className: 'col s12',
                label: t('map', 'styleUrl'),
                initialValue: settings.styleUrl,
                oninput: (value: string) => change({ styleUrl: value }),
              }),
            ])
          : undefined,
        settings.basemapKind === 'pmtiles'
          ? m('.row', [
              m(TextInput, {
                className: 'col s12',
                label: t('map', 'pmtilesPath'),
                initialValue: pathInputFromLocation(settings.pmtilesLocation),
                oninput: (value: string) =>
                  change({ pmtilesLocation: locationFromPathInput(value) }),
              }),
            ])
          : undefined,
        settings.basemapKind === 'style'
          ? undefined
          : m('.row', [
              m(TextInput, {
                className: 'col s12',
                label: t('map', 'attribution'),
                initialValue: settings.attribution,
                oninput: (value: string) => change({ attribution: value }),
              }),
            ]),
        m('p.fm-map-network-notice', networkNotice(settings)),
        problem === undefined
          ? undefined
          : m('p.fm-field-error', { role: 'alert' }, t('map', problem)),
        m('.row', [
          m(
            'button.btn-flat.fm-map-reset',
            { type: 'button', onclick: () => onChange(DEFAULT_MAP_SETTINGS) },
            t('map', 'resetToDefault'),
          ),
          client === undefined
            ? undefined
            : m(
                'button.btn-flat.fm-map-preview-toggle',
                {
                  type: 'button',
                  disabled: problem !== undefined,
                  onclick: () => {
                    previewOpen = !previewOpen;
                  },
                },
                previewOpen ? t('map', 'hidePreview') : t('map', 'showPreview'),
              ),
        ]),
        client !== undefined && previewOpen && problem === undefined
          ? m(
              '.fm-map-preview',
              m(MapView, { client, settings, dark: resolvedTheme() === 'dark', markers: [] }),
            )
          : undefined,
      ];
    },
  };
};
