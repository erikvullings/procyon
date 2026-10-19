import m, { type FactoryComponent } from 'mithril';
import { NumberInput, Select, Switch, TextInput } from 'mithril-materialized';
import { t } from '../../i18n';
import type { MapBasemapKind, MapSettings } from '../../models';
import { resolvedTheme } from '../../utilities/resolved-theme';
import { applyPreset, MAP_PRESETS, presetFor } from './map-presets';
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
  return {
    view: ({ attrs: { client, settings, onChange } }) => {
      const change = (patch: Partial<MapSettings>): void => onChange({ ...settings, ...patch });
      const problem = validateMapSettings(settings);
      const preset = presetFor(settings);
      const raster = settings.basemapKind === 'raster';
      return m.fragment({}, [
        m('.row', m('h4.fm-settings-section-heading.col.s12', t('map', 'sectionTitle'))),
        m('.row', [
          m(Switch, {
            className: 'col s12 m4',
            label: t('map', 'onlineTiles'),
            checked: settings.onlineTiles,
            left: t('settings', 'off'),
            right: t('settings', 'on'),
            onchange: (onlineTiles: boolean) => change({ onlineTiles }),
          }),
          m(Select<string>, {
            className: 'col s12 m8',
            label: t('map', 'preset'),
            placeholder: t('map', 'presetCustom'),
            options: MAP_PRESETS.map((entry) => ({
              id: entry.id,
              label: `${entry.name} - ${entry.note}`,
            })),
            ...(preset === undefined ? {} : { checkedId: preset.id }),
            onchange: ([value]) => {
              const chosen = MAP_PRESETS.find((entry) => entry.id === value);
              if (chosen !== undefined) onChange(applyPreset(settings, chosen));
            },
          }),
        ]),
        m('.row', [
          m(Select<MapBasemapKind>, {
            className: 'col s12 m4',
            label: t('map', 'basemapKind'),
            options: (['raster', 'style', 'pmtiles'] as const).map((id) => ({
              id,
              label: t('map', KIND_LABELS[id]),
            })),
            checkedId: settings.basemapKind,
            onchange: ([value]) => value !== undefined && change({ basemapKind: value }),
          }),
          raster
            ? m(TextInput, {
                className: 'col s12 m8',
                label: t('map', 'urlTemplate'),
                helperText: t('map', 'urlTemplateHint'),
                value: settings.urlTemplate,
                oninput: (value: string) => change({ urlTemplate: value }),
              })
            : undefined,
          settings.basemapKind === 'style'
            ? m(TextInput, {
                className: 'col s12 m8',
                label: t('map', 'styleUrl'),
                value: settings.styleUrl,
                oninput: (value: string) => change({ styleUrl: value }),
              })
            : undefined,
          settings.basemapKind === 'pmtiles'
            ? m(TextInput, {
                className: 'col s12 m8',
                label: t('map', 'pmtilesPath'),
                value: pathInputFromLocation(settings.pmtilesLocation),
                oninput: (value: string) =>
                  change({ pmtilesLocation: locationFromPathInput(value) }),
              })
            : undefined,
        ]),
        settings.basemapKind === 'style'
          ? undefined
          : m('.row', [
              m(TextInput, {
                className: raster ? 'col s12 m8' : 'col s12',
                label: t('map', 'attribution'),
                value: settings.attribution,
                oninput: (value: string) => change({ attribution: value }),
              }),
              raster
                ? m(NumberInput, {
                    className: 'col s12 m4',
                    label: t('map', 'maxZoom'),
                    value: settings.maxZoom,
                    min: 1,
                    max: 24,
                    oninput: (value: number) => change({ maxZoom: value }),
                  })
                : undefined,
            ]),
        m('.fm-map-settings-footer', [
          m('p.fm-map-network-notice', networkNotice(settings)),
          m(
            'button.btn-flat.fm-map-reset',
            { type: 'button', onclick: () => onChange(DEFAULT_MAP_SETTINGS) },
            t('map', 'resetToDefault'),
          ),
        ]),
        problem === undefined
          ? undefined
          : m('p.fm-field-error', { role: 'alert' }, t('map', problem)),
        client !== undefined && problem === undefined
          ? m(
              '.fm-map-preview',
              m(MapView, {
                client,
                settings,
                directTiles: true,
                dark: resolvedTheme() === 'dark',
                markers: [],
              }),
            )
          : undefined,
      ]);
    },
  };
};
