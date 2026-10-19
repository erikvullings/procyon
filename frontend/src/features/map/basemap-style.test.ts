import { describe, expect, it } from 'vitest';
import type { MapSettings } from '../../models';
import { blankGridStyle, buildBasemapStyle, graticule } from './basemap-style';
import { DEFAULT_MAP_SETTINGS } from './map-settings';

const sources = { tileTemplate: '/api/v1/map/tiles/{z}/{x}/{y}', pmtilesUrl: undefined };

function hasRemoteUrl(value: unknown): boolean {
  return /https?:\/\//.test(JSON.stringify(value));
}

describe('buildBasemapStyle', () => {
  it('points the raster source at the backend proxy with attribution and max zoom', () => {
    const style = buildBasemapStyle(DEFAULT_MAP_SETTINGS, sources, false);
    expect(typeof style).toBe('object');
    expect(style).toMatchObject({
      sources: {
        basemap: {
          type: 'raster',
          tiles: ['/api/v1/map/tiles/{z}/{x}/{y}'],
          maxzoom: 19,
          attribution: DEFAULT_MAP_SETTINGS.attribution,
        },
      },
    });
    expect(hasRemoteUrl(style)).toBe(false);
  });

  it('draws a blank grid and names no URL when online tiles are off', () => {
    for (const kind of ['raster', 'style'] as const) {
      const settings: MapSettings = {
        ...DEFAULT_MAP_SETTINGS,
        basemapKind: kind,
        styleUrl: 'https://maps.example/style.json',
        onlineTiles: false,
      };
      const style = buildBasemapStyle(settings, sources, false);
      expect(typeof style).toBe('object');
      expect(hasRemoteUrl(style)).toBe(false);
      expect(JSON.stringify(style)).toContain('graticule');
    }
  });

  it('hands a remote style URL to MapLibre when online', () => {
    expect(
      buildBasemapStyle(
        { ...DEFAULT_MAP_SETTINGS, basemapKind: 'style', styleUrl: 'https://maps.example/s.json' },
        sources,
        false,
      ),
    ).toBe('https://maps.example/s.json');
  });

  it('reads PMTiles through the pmtiles protocol even when online tiles are off', () => {
    const style = buildBasemapStyle(
      { ...DEFAULT_MAP_SETTINGS, basemapKind: 'pmtiles', onlineTiles: false },
      { tileTemplate: undefined, pmtilesUrl: '/api/v1/media?location=x' },
      false,
    );
    expect(style).toMatchObject({
      sources: { basemap: { url: 'pmtiles:///api/v1/media?location=x' } },
    });
  });

  it('falls back to the grid when a source cannot be resolved', () => {
    expect(
      buildBasemapStyle(
        { ...DEFAULT_MAP_SETTINGS, basemapKind: 'pmtiles' },
        { tileTemplate: undefined, pmtilesUrl: undefined },
        true,
      ),
    ).toEqual(blankGridStyle(true));
  });
});

describe('graticule', () => {
  it('covers the world in lines', () => {
    expect(graticule(30).features.length).toBeGreaterThan(10);
  });
});
