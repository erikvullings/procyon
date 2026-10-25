import type { EnglishCatalogue } from '../../i18n';
import type { MapSettings } from '../../models';
import { DEFAULT_MAP_SETTINGS } from './map-settings';

/** A ready-made free basemap. Every URL was checked to answer without an API key. */
export interface MapPreset {
  readonly id: string;
  readonly name: string;
  /** Short note on coverage or terms shown in the picker. */
  readonly note: string;
  readonly settings: Pick<
    MapSettings,
    'basemapKind' | 'urlTemplate' | 'styleUrl' | 'attribution' | 'maxZoom'
  >;
}

/** Catalogue key (`mapPreset` group) of a preset's note: its id in camelCase. */
export function presetNoteKey(id: string): keyof EnglishCatalogue['mapPreset'] {
  return id.replaceAll(/-(\w)/gu, (_match, letter: string) =>
    letter.toUpperCase(),
  ) as keyof EnglishCatalogue['mapPreset'];
}

const PDOK_ATTRIBUTION = '© Kadaster / PDOK';
const OSM_ATTRIBUTION = '© OpenStreetMap contributors';

const raster = (
  id: string,
  name: string,
  note: string,
  urlTemplate: string,
  attribution: string,
  maxZoom: number,
): MapPreset => ({
  id,
  name,
  note,
  settings: { basemapKind: 'raster', urlTemplate, styleUrl: '', attribution, maxZoom },
});

const vector = (
  id: string,
  name: string,
  note: string,
  styleUrl: string,
  attribution: string,
): MapPreset => ({
  id,
  name,
  note,
  settings: {
    basemapKind: 'style',
    urlTemplate: DEFAULT_MAP_SETTINGS.urlTemplate,
    styleUrl,
    attribution,
    maxZoom: DEFAULT_MAP_SETTINGS.maxZoom,
  },
});

/**
 * Free basemaps that need no account. Public servers have fair-use limits: for heavy use, run
 * your own tile server or use a local PMTiles file.
 */
export const MAP_PRESETS: readonly MapPreset[] = [
  raster(
    'osm',
    'OpenStreetMap Standard',
    'Raster, worldwide. Public servers: light use only.',
    'https://tile.openstreetmap.org/{z}/{x}/{y}.png',
    OSM_ATTRIBUTION,
    19,
  ),
  raster(
    'osm-de',
    'OpenStreetMap Germany',
    'Raster, worldwide, local labels.',
    'https://tile.openstreetmap.de/{z}/{x}/{y}.png',
    OSM_ATTRIBUTION,
    18,
  ),
  raster(
    'opentopomap',
    'OpenTopoMap',
    'Raster, contour lines and terrain.',
    'https://tile.opentopomap.org/{z}/{x}/{y}.png',
    `${OSM_ATTRIBUTION}, SRTM | © OpenTopoMap (CC-BY-SA)`,
    17,
  ),
  raster(
    'cyclosm',
    'CyclOSM',
    'Raster, cycling routes and infrastructure.',
    'https://a.tile-cyclosm.openstreetmap.fr/cyclosm/{z}/{x}/{y}.png',
    `${OSM_ATTRIBUTION} | © CyclOSM`,
    20,
  ),
  raster(
    'pdok-brt',
    'PDOK BRT Achtergrondkaart (NL)',
    'Raster, Netherlands, official topographic map.',
    'https://service.pdok.nl/brt/achtergrondkaart/wmts/v2_0/standaard/EPSG:3857/{z}/{x}/{y}.png',
    PDOK_ATTRIBUTION,
    19,
  ),
  raster(
    'pdok-brt-grey',
    'PDOK BRT Achtergrondkaart grey (NL)',
    'Raster, Netherlands, grey tones for overlays.',
    'https://service.pdok.nl/brt/achtergrondkaart/wmts/v2_0/grijs/EPSG:3857/{z}/{x}/{y}.png',
    PDOK_ATTRIBUTION,
    19,
  ),
  raster(
    'pdok-aerial',
    'PDOK Aerial photo (NL)',
    'Raster, Netherlands, current aerial imagery.',
    'https://service.pdok.nl/hwh/luchtfotorgb/wmts/v1_0/Actueel_orthoHR/EPSG:3857/{z}/{x}/{y}.jpeg',
    `${PDOK_ATTRIBUTION} / Beeldmateriaal.nl`,
    19,
  ),
  vector(
    'pdok-brt-vector',
    'PDOK BRT Achtergrondkaart vector (NL)',
    'Vector style, Netherlands. Sharp at every zoom.',
    'https://api.pdok.nl/kadaster/brt-achtergrondkaart/ogc/v1/styles/standaard__webmercatorquad?f=json',
    PDOK_ATTRIBUTION,
  ),
  vector(
    'openfreemap-liberty',
    'OpenFreeMap Liberty',
    'Vector style, worldwide, no limits or key.',
    'https://tiles.openfreemap.org/styles/liberty',
    `${OSM_ATTRIBUTION} © OpenMapTiles`,
  ),
  vector(
    'openfreemap-positron',
    'OpenFreeMap Positron',
    'Vector style, worldwide, light.',
    'https://tiles.openfreemap.org/styles/positron',
    `${OSM_ATTRIBUTION} © OpenMapTiles`,
  ),
  vector(
    'versatiles',
    'VersaTiles Colorful',
    'Vector style, worldwide, community server.',
    'https://tiles.versatiles.org/assets/styles/colorful/style.json',
    OSM_ATTRIBUTION,
  ),
];

/** The preset whose address matches the settings, if any. */
export function presetFor(settings: MapSettings): MapPreset | undefined {
  return MAP_PRESETS.find((preset) =>
    preset.settings.basemapKind === 'raster'
      ? settings.basemapKind === 'raster' && settings.urlTemplate === preset.settings.urlTemplate
      : settings.basemapKind === 'style' && settings.styleUrl === preset.settings.styleUrl,
  );
}

/** Settings after choosing `preset`; the online switch and offline file choices are kept. */
export function applyPreset(settings: MapSettings, preset: MapPreset): MapSettings {
  return {
    ...settings,
    ...preset.settings,
    remoteHost: new URL(
      (preset.settings.basemapKind === 'raster'
        ? preset.settings.urlTemplate
        : preset.settings.styleUrl
      )
        .replaceAll('{z}', '0')
        .replaceAll('{x}', '0')
        .replaceAll('{y}', '0'),
    ).host,
  };
}
