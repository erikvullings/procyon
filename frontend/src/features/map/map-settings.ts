import type { Location, MapSettings } from '../../models';

export const DEFAULT_TILE_TEMPLATE = 'https://tile.openstreetmap.org/{z}/{x}/{y}.png';
export const DEFAULT_ATTRIBUTION = '© OpenStreetMap contributors';

/** The free default: OpenStreetMap's public raster tiles. */
export const DEFAULT_MAP_SETTINGS: MapSettings = {
  onlineTiles: true,
  basemapKind: 'raster',
  urlTemplate: DEFAULT_TILE_TEMPLATE,
  styleUrl: '',
  pmtilesLocation: null,
  attribution: DEFAULT_ATTRIBUTION,
  maxZoom: 19,
  remoteHost: 'tile.openstreetmap.org',
};

/** i18n keys of the `map` group describing why the settings cannot be saved. */
export type MapSettingsProblem =
  | 'urlInvalid'
  | 'urlPlaceholders'
  | 'attributionRequired'
  | 'zoomRange'
  | 'pmtilesFile';

function parseUrl(text: string, withPlaceholders: boolean): URL | undefined {
  try {
    return new URL(
      withPlaceholders
        ? text.replaceAll('{z}', '0').replaceAll('{x}', '0').replaceAll('{y}', '0')
        : text,
    );
  } catch {
    return undefined;
  }
}

function acceptableUrl(url: URL | undefined): url is URL {
  if (url === undefined || url.username !== '' || url.password !== '' || url.hostname === '') {
    return false;
  }
  const local = ['localhost', '127.0.0.1', '[::1]'].includes(url.hostname);
  return url.protocol === 'https:' || (url.protocol === 'http:' && local);
}

/** Mirrors the backend's rules so the editor can explain a problem before saving. */
export function validateMapSettings(settings: MapSettings): MapSettingsProblem | undefined {
  if (settings.basemapKind === 'raster') {
    for (const placeholder of ['{z}', '{x}', '{y}']) {
      if (!settings.urlTemplate.includes(placeholder)) return 'urlPlaceholders';
    }
    if (!acceptableUrl(parseUrl(settings.urlTemplate, true))) return 'urlInvalid';
    if (settings.attribution.trim() === '') return 'attributionRequired';
    if (!Number.isInteger(settings.maxZoom) || settings.maxZoom < 1 || settings.maxZoom > 24) {
      return 'zoomRange';
    }
    return undefined;
  }
  if (settings.basemapKind === 'style') {
    return acceptableUrl(parseUrl(settings.styleUrl, false)) ? undefined : 'urlInvalid';
  }
  if (!settings.pmtilesLocation?.uri.toLowerCase().endsWith('.pmtiles')) return 'pmtilesFile';
  return settings.attribution.trim() === '' ? 'attributionRequired' : undefined;
}

/** The remote host a basemap contacts, for the "requests go to ..." notice; undefined for files. */
export function remoteHostOf(settings: MapSettings): string | undefined {
  if (settings.basemapKind === 'pmtiles') return undefined;
  const url =
    settings.basemapKind === 'raster'
      ? parseUrl(settings.urlTemplate, true)
      : parseUrl(settings.styleUrl, false);
  return url?.host === '' ? undefined : url?.host;
}

/** Accepts a `file://` URI or an absolute path and returns the matching local location. */
export function locationFromPathInput(text: string): Location | null {
  const trimmed = text.trim();
  if (trimmed === '') return null;
  if (trimmed.startsWith('file://')) return { providerId: 'local', uri: trimmed };
  if (/^[A-Za-z]:[\\/]/.test(trimmed)) {
    return { providerId: 'local', uri: `file:///${encodeURI(trimmed.replaceAll('\\', '/'))}` };
  }
  if (trimmed.startsWith('/')) return { providerId: 'local', uri: `file://${encodeURI(trimmed)}` };
  return null;
}

/** The path shown in the text field for a stored location. */
export function pathInputFromLocation(location: Location | null | undefined): string {
  if (location == null) return '';
  try {
    return decodeURI(location.uri.replace(/^file:\/\//, ''));
  } catch {
    return location.uri;
  }
}
