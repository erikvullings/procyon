import type { LayerSpecification, SourceSpecification, StyleSpecification } from 'maplibre-gl';
import type { MapSettings } from '../../models';

/** Longitude/latitude lines every 10 degrees, drawn when no map imagery is available. */
export function graticule(stepDegrees = 10): GeoJSON.FeatureCollection<GeoJSON.LineString> {
  const features: GeoJSON.Feature<GeoJSON.LineString>[] = [];
  for (let lon = -180; lon <= 180; lon += stepDegrees) {
    features.push({
      type: 'Feature',
      properties: {},
      geometry: {
        type: 'LineString',
        coordinates: [
          [lon, -85],
          [lon, 85],
        ],
      },
    });
  }
  for (let lat = -80; lat <= 80; lat += stepDegrees) {
    features.push({
      type: 'Feature',
      properties: {},
      geometry: {
        type: 'LineString',
        coordinates: [
          [-180, lat],
          [180, lat],
        ],
      },
    });
  }
  return { type: 'FeatureCollection', features };
}

/** What the style builder needs besides the settings: URLs only the client can resolve. */
export interface BasemapSources {
  /** Backend tile proxy template for raster basemaps. */
  readonly tileTemplate: string | undefined;
  /** Range-capable URL of the local PMTiles file. */
  readonly pmtilesUrl: string | undefined;
}

/** A style with no imagery: a plain background and a graticule. */
export function blankGridStyle(dark: boolean): StyleSpecification {
  const background: LayerSpecification = {
    id: 'background',
    type: 'background',
    paint: { 'background-color': dark ? 'rgb(27, 32, 38)' : 'rgb(238, 241, 244)' },
  };
  const lines: LayerSpecification = {
    id: 'graticule',
    type: 'line',
    source: 'graticule',
    paint: { 'line-color': dark ? 'rgb(58, 67, 77)' : 'rgb(201, 208, 216)', 'line-width': 1 },
  };
  return {
    version: 8,
    sources: { graticule: { type: 'geojson', data: graticule() } },
    layers: [background, lines],
  };
}

/**
 * Builds what MapLibre should load for the settings: a style document, or the URL of a remote
 * style. Nothing here performs I/O; with online tiles off, remote kinds fall back to the grid so
 * no request can leave the machine.
 */
export function buildBasemapStyle(
  settings: MapSettings,
  sources: BasemapSources,
  dark: boolean,
): StyleSpecification | string {
  if (settings.basemapKind === 'style') {
    return settings.onlineTiles && settings.styleUrl !== ''
      ? settings.styleUrl
      : blankGridStyle(dark);
  }
  let source: SourceSpecification | undefined;
  if (settings.basemapKind === 'raster') {
    if (settings.onlineTiles && sources.tileTemplate !== undefined) {
      source = {
        type: 'raster',
        tiles: [sources.tileTemplate],
        tileSize: 256,
        maxzoom: settings.maxZoom,
        attribution: settings.attribution,
      };
    }
  } else if (sources.pmtilesUrl !== undefined) {
    source = {
      type: 'raster',
      url: `pmtiles://${sources.pmtilesUrl}`,
      tileSize: 256,
      attribution: settings.attribution,
    };
  }
  if (source === undefined) return blankGridStyle(dark);
  return {
    version: 8,
    sources: { basemap: source },
    layers: [{ id: 'basemap', type: 'raster', source: 'basemap' }],
  };
}
