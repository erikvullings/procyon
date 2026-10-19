import { describe, expect, it } from 'vitest';
import type { MapSettings } from '../../models';
import {
  DEFAULT_MAP_SETTINGS,
  locationFromPathInput,
  pathInputFromLocation,
  remoteHostOf,
  validateMapSettings,
} from './map-settings';

const raster = (urlTemplate: string, patch: Partial<MapSettings> = {}): MapSettings => ({
  ...DEFAULT_MAP_SETTINGS,
  urlTemplate,
  ...patch,
});

describe('validateMapSettings', () => {
  it('accepts the OpenStreetMap default', () => {
    expect(validateMapSettings(DEFAULT_MAP_SETTINGS)).toBeUndefined();
  });

  it('requires placeholders, https (or localhost), no credentials, attribution and a sane zoom', () => {
    expect(validateMapSettings(raster('https://t.example/{z}/{x}.png'))).toBe('urlPlaceholders');
    expect(validateMapSettings(raster('http://t.example/{z}/{x}/{y}.png'))).toBe('urlInvalid');
    expect(validateMapSettings(raster('http://localhost:8080/{z}/{x}/{y}.png'))).toBeUndefined();
    expect(validateMapSettings(raster('https://u:p@t.example/{z}/{x}/{y}.png'))).toBe('urlInvalid');
    expect(
      validateMapSettings(raster('https://t.example/{z}/{x}/{y}.png', { attribution: ' ' })),
    ).toBe('attributionRequired');
    expect(validateMapSettings(raster('https://t.example/{z}/{x}/{y}.png', { maxZoom: 40 }))).toBe(
      'zoomRange',
    );
  });

  it('checks style URLs and PMTiles files', () => {
    expect(
      validateMapSettings({ ...DEFAULT_MAP_SETTINGS, basemapKind: 'style', styleUrl: 'ftp://x' }),
    ).toBe('urlInvalid');
    expect(
      validateMapSettings({
        ...DEFAULT_MAP_SETTINGS,
        basemapKind: 'style',
        styleUrl: 'https://maps.example/style.json',
      }),
    ).toBeUndefined();
    expect(validateMapSettings({ ...DEFAULT_MAP_SETTINGS, basemapKind: 'pmtiles' })).toBe(
      'pmtilesFile',
    );
    expect(
      validateMapSettings({
        ...DEFAULT_MAP_SETTINGS,
        basemapKind: 'pmtiles',
        pmtilesLocation: { providerId: 'local', uri: 'file:///maps/world.PMTiles' },
      }),
    ).toBeUndefined();
  });
});

describe('remoteHostOf', () => {
  it('names the host that receives requests, and none for local files', () => {
    expect(remoteHostOf(DEFAULT_MAP_SETTINGS)).toBe('tile.openstreetmap.org');
    expect(remoteHostOf({ ...DEFAULT_MAP_SETTINGS, basemapKind: 'pmtiles' })).toBeUndefined();
    expect(
      remoteHostOf({
        ...DEFAULT_MAP_SETTINGS,
        basemapKind: 'style',
        styleUrl: 'https://s.example/a.json',
      }),
    ).toBe('s.example');
  });
});

describe('PMTiles path input', () => {
  it('turns paths and file URIs into local locations and back', () => {
    expect(locationFromPathInput('/maps/my world.pmtiles')).toEqual({
      providerId: 'local',
      uri: 'file:///maps/my%20world.pmtiles',
    });
    expect(locationFromPathInput('C:\\maps\\w.pmtiles')?.uri).toBe('file:///C:/maps/w.pmtiles');
    expect(locationFromPathInput('file:///a.pmtiles')?.uri).toBe('file:///a.pmtiles');
    expect(locationFromPathInput('relative.pmtiles')).toBeNull();
    expect(
      pathInputFromLocation({ providerId: 'local', uri: 'file:///maps/my%20world.pmtiles' }),
    ).toBe('/maps/my world.pmtiles');
  });
});
