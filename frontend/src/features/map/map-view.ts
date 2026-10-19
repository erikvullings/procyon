import './map-view.css';
import type * as MapLibre from 'maplibre-gl';
import m, { type FactoryComponent } from 'mithril';
import { t } from '../../i18n';
import type { Location, MapSettings } from '../../models';
import { type BasemapSources, buildBasemapStyle } from './basemap-style';
import { boundsOf, type MapMarker, MarkerIndex } from './marker-index';

/** What the map needs from the file-manager client. */
export interface MapViewClient {
  mapTileUrlTemplate(): Promise<string>;
  mediaUrl(location: Location): Promise<string>;
}

export interface MapPosition {
  readonly latitude: number;
  readonly longitude: number;
}

export interface MapViewAttrs {
  readonly client: MapViewClient;
  readonly settings: MapSettings;
  readonly dark: boolean;
  readonly markers: readonly MapMarker[];
  readonly selectedIds?: ReadonlySet<string>;
  /** Re-fits the viewport to the markers whenever this value changes (and once on load). */
  readonly fitKey?: string | number;
  /** Builds the DOM of a single marker; the default is a round pin. */
  readonly renderMarker?: (marker: MapMarker, selected: boolean) => HTMLElement;
  /** Content of the popup opened by clicking a marker; no popup when it returns undefined. */
  readonly renderPopup?: (marker: MapMarker) => string | HTMLElement | undefined;
  readonly onMarkerClick?: (marker: MapMarker) => void;
  readonly onMapClick?: (position: MapPosition) => void;
  readonly onMarkerDrag?: (marker: MapMarker, position: MapPosition) => void;
  /** Vector data drawn under the markers: points, lines and polygons with simplestyle colours. */
  readonly features?: GeoJSON.FeatureCollection<GeoJSON.Geometry | null>;
  /** Extent of `features`; the map fits to it (when there are no markers) on load and fitKey changes. */
  readonly dataBounds?: readonly [number, number, number, number];
  /** Index (in `features.features`) of the feature that was clicked, or undefined for empty space. */
  readonly onFeatureClick?: (index: number | undefined) => void;
  /** Index of the feature to emphasise. */
  readonly highlightIndex?: number;
  readonly ariaLabel?: string;
}

type Status = 'loading' | 'ready' | 'failed';

interface DrawnItem {
  readonly marker: MapLibre.Marker;
  readonly element: HTMLElement;
  readonly mapMarker: MapMarker | undefined;
}

/** Wide enough to start with the whole world visible. */
const INITIAL_CENTER: [number, number] = [0, 20];
const INITIAL_ZOOM = 1.5;
const FIT_PADDING = 48;
const FIT_MAX_ZOOM = 16;

function defaultMarkerElement(marker: MapMarker, selected: boolean): HTMLElement {
  const element = document.createElement('button');
  element.type = 'button';
  element.className = 'fm-map-marker';
  element.classList.toggle('is-selected', selected);
  if (marker.title !== undefined) element.title = marker.title;
  element.setAttribute('aria-label', marker.title ?? marker.id);
  return element;
}

function clusterElement(count: number): HTMLElement {
  const element = document.createElement('button');
  element.type = 'button';
  element.className = 'fm-map-cluster';
  element.textContent = String(count);
  element.title = t('map', 'cluster', { n: count });
  element.setAttribute('aria-label', t('map', 'cluster', { n: count }));
  return element;
}

const FEATURE_SOURCE = 'fm-features';
const FEATURE_LAYERS = ['fm-features-fill', 'fm-features-line', 'fm-features-circle'] as const;
const HIGHLIGHT_LAYER = 'fm-features-highlight';
const DEFAULT_FEATURE_COLOUR = 'rgb(25, 118, 210)';

/** Copies features with their position in the list, so a click can be mapped back to the source. */
export function withFeatureIndexes(
  collection: GeoJSON.FeatureCollection<GeoJSON.Geometry | null>,
): GeoJSON.FeatureCollection {
  // Features without geometry are valid GeoJSON and simply draw nothing.
  return {
    type: 'FeatureCollection',
    features: collection.features.map((feature, index) => ({
      ...feature,
      properties: { ...(feature.properties ?? {}), _fmIndex: index },
    })) as GeoJSON.Feature[],
  };
}

function basemapKey(settings: MapSettings, dark: boolean): string {
  const blank = settings.basemapKind === 'style' ? false : !settings.onlineTiles;
  return JSON.stringify([settings, blank ? dark : null]);
}

/**
 * Shared map (task 0275): MapLibre GL JS loaded lazily, a replaceable basemap, clustered markers
 * with popups, fit-to-bounds and light/dark theming. The library is imported only when a map is
 * mounted, and no tile request is made before that. Markers are DOM overlays so features can use
 * thumbnails or any other content. Viewport state (centre, zoom) lives in the map engine; all
 * data comes in through attrs.
 */
export const MapView: FactoryComponent<MapViewAttrs> = (initial) => {
  let latest: MapViewAttrs = initial.attrs;
  let status: Status = 'loading';
  let destroyed = false;
  let lib: typeof MapLibre | undefined;
  let map: MapLibre.Map | undefined;
  let index: MarkerIndex | undefined;
  let indexedMarkers: readonly MapMarker[] | undefined;
  let appliedBasemap = '';
  let appliedFitKey: string | number | undefined;
  let hasFitted = false;
  let popup: MapLibre.Popup | undefined;
  const drawn = new Map<string, DrawnItem>();
  let pmtilesProtocolRegistered = false;

  async function resolveSources(attrs: MapViewAttrs): Promise<BasemapSources> {
    const { settings } = attrs;
    const tileTemplate =
      settings.basemapKind === 'raster' && settings.onlineTiles
        ? await attrs.client.mapTileUrlTemplate()
        : undefined;
    const pmtilesUrl =
      settings.basemapKind === 'pmtiles' && settings.pmtilesLocation != null
        ? await attrs.client.mediaUrl(settings.pmtilesLocation)
        : undefined;
    return { tileTemplate, pmtilesUrl };
  }

  async function ensurePmtilesProtocol(library: typeof MapLibre): Promise<void> {
    if (pmtilesProtocolRegistered) return;
    const { Protocol } = await import('pmtiles');
    const protocol = new Protocol();
    library.addProtocol('pmtiles', protocol.tile);
    pmtilesProtocolRegistered = true;
  }

  function clearDrawn(): void {
    for (const item of drawn.values()) item.marker.remove();
    drawn.clear();
    popup?.remove();
    popup = undefined;
  }

  function fit(): void {
    if (map === undefined || lib === undefined) return;
    const bounds = boundsOf(latest.markers) ?? latest.dataBounds;
    if (bounds === undefined) return;
    const [west, south, east, north] = bounds;
    if (west === east && south === north) {
      map.jumpTo({ center: [west, south], zoom: Math.min(14, latest.settings.maxZoom) });
    } else {
      map.fitBounds(
        [
          [west, south],
          [east, north],
        ],
        { padding: FIT_PADDING, maxZoom: FIT_MAX_ZOOM, duration: 0 },
      );
    }
  }

  function applySelection(): void {
    const selected = latest.selectedIds ?? new Set<string>();
    for (const item of drawn.values()) {
      if (item.mapMarker !== undefined) {
        item.element.classList.toggle('is-selected', selected.has(item.mapMarker.id));
      }
    }
  }

  function openPopup(mapMarker: MapMarker): void {
    popup?.remove();
    popup = undefined;
    const content = latest.renderPopup?.(mapMarker);
    if (content === undefined || map === undefined || lib === undefined) return;
    const next = new lib.Popup({ offset: 16, closeButton: true });
    next.setLngLat([mapMarker.longitude, mapMarker.latitude]);
    if (typeof content === 'string') next.setText(content);
    else next.setDOMContent(content);
    next.addTo(map);
    popup = next;
  }

  function drawViewport(): void {
    if (map === undefined || lib === undefined || index === undefined) return;
    const library = lib;
    const view = map.getBounds();
    const items = index.viewport(
      [view.getWest(), view.getSouth(), view.getEast(), view.getNorth()],
      map.getZoom(),
    );
    const wanted = new Set(items.map((item) => item.key));
    for (const [key, item] of drawn) {
      if (!wanted.has(key)) {
        item.marker.remove();
        drawn.delete(key);
      }
    }
    const selected = latest.selectedIds ?? new Set<string>();
    for (const item of items) {
      if (drawn.has(item.key)) continue;
      if (item.kind === 'cluster') {
        const element = clusterElement(item.count);
        element.addEventListener('click', (event) => {
          event.stopPropagation();
          if (index === undefined || map === undefined) return;
          map.easeTo({
            center: [item.longitude, item.latitude],
            zoom: Math.min(index.expansionZoom(item.clusterId), latest.settings.maxZoom),
          });
        });
        const marker = new library.Marker({ element })
          .setLngLat([item.longitude, item.latitude])
          .addTo(map);
        drawn.set(item.key, { marker, element, mapMarker: undefined });
        continue;
      }
      const mapMarker = item.marker;
      const element =
        latest.renderMarker?.(mapMarker, selected.has(mapMarker.id)) ??
        defaultMarkerElement(mapMarker, selected.has(mapMarker.id));
      element.classList.toggle('is-selected', selected.has(mapMarker.id));
      element.addEventListener('click', (event) => {
        event.stopPropagation();
        openPopup(mapMarker);
        latest.onMarkerClick?.(mapMarker);
      });
      const marker = new library.Marker({
        element,
        draggable: mapMarker.draggable === true,
      })
        .setLngLat([mapMarker.longitude, mapMarker.latitude])
        .addTo(map);
      if (mapMarker.draggable === true) {
        marker.on('dragend', () => {
          const position = marker.getLngLat();
          latest.onMarkerDrag?.(mapMarker, { latitude: position.lat, longitude: position.lng });
        });
      }
      drawn.set(item.key, { marker, element, mapMarker });
    }
  }

  let appliedFeatures: GeoJSON.FeatureCollection<GeoJSON.Geometry | null> | undefined;
  let appliedHighlight: number | undefined;

  /** Adds the vector data source and layers; safe to call again after a style swap. */
  function ensureFeatureLayers(): void {
    const target = map;
    if (target === undefined) return;
    if (target.getSource(FEATURE_SOURCE) === undefined) {
      target.addSource(FEATURE_SOURCE, {
        type: 'geojson',
        data: withFeatureIndexes(latest.features ?? { type: 'FeatureCollection', features: [] }),
      });
      appliedFeatures = latest.features;
      const colour = (key: string): unknown => ['coalesce', ['get', key], DEFAULT_FEATURE_COLOUR];
      target.addLayer({
        id: 'fm-features-fill',
        type: 'fill',
        source: FEATURE_SOURCE,
        filter: ['==', ['geometry-type'], 'Polygon'],
        paint: {
          'fill-color': colour('fill') as never,
          'fill-opacity': ['coalesce', ['get', 'fill-opacity'], 0.3] as never,
        },
      });
      target.addLayer({
        id: 'fm-features-line',
        type: 'line',
        source: FEATURE_SOURCE,
        filter: ['in', ['geometry-type'], ['literal', ['LineString', 'Polygon']]],
        paint: {
          'line-color': colour('stroke') as never,
          'line-width': ['coalesce', ['get', 'stroke-width'], 2] as never,
          'line-opacity': ['coalesce', ['get', 'stroke-opacity'], 1] as never,
        },
      });
      target.addLayer({
        id: 'fm-features-circle',
        type: 'circle',
        source: FEATURE_SOURCE,
        filter: ['==', ['geometry-type'], 'Point'],
        paint: {
          'circle-color': colour('marker-color') as never,
          'circle-radius': ['match', ['get', 'marker-size'], 'small', 4, 'large', 9, 6] as never,
          'circle-stroke-width': 1.5,
          'circle-stroke-color': 'rgb(255, 255, 255)',
        },
      });
      target.addLayer({
        id: HIGHLIGHT_LAYER,
        type: 'line',
        source: FEATURE_SOURCE,
        filter: ['==', ['get', '_fmIndex'], -1],
        paint: { 'line-color': 'rgb(255, 160, 0)', 'line-width': 5, 'line-opacity': 0.8 },
      });
      appliedHighlight = undefined;
    }
    applyHighlight();
  }

  function applyHighlight(): void {
    if (map === undefined || map.getLayer(HIGHLIGHT_LAYER) === undefined) return;
    if (appliedHighlight === latest.highlightIndex) return;
    appliedHighlight = latest.highlightIndex;
    map.setFilter(HIGHLIGHT_LAYER, ['==', ['get', '_fmIndex'], latest.highlightIndex ?? -1]);
  }

  function syncFeatures(): void {
    if (map === undefined) return;
    ensureFeatureLayers();
    if (appliedFeatures !== latest.features) {
      appliedFeatures = latest.features;
      const source = map.getSource(FEATURE_SOURCE) as MapLibre.GeoJSONSource | undefined;
      source?.setData(
        withFeatureIndexes(latest.features ?? { type: 'FeatureCollection', features: [] }),
      );
    }
    applyHighlight();
  }

  function syncMarkers(): void {
    if (indexedMarkers === latest.markers) return;
    indexedMarkers = latest.markers;
    index = new MarkerIndex(latest.markers, latest.settings.maxZoom);
    clearDrawn();
    drawViewport();
  }

  /** Resolves the sources the settings need and builds what MapLibre should load. */
  async function resolveStyle(): Promise<ReturnType<typeof buildBasemapStyle> | undefined> {
    const library = lib;
    if (library === undefined) return undefined;
    const attrs = latest;
    appliedBasemap = basemapKey(attrs.settings, attrs.dark);
    const sources = await resolveSources(attrs);
    if (destroyed) return undefined;
    if (attrs.settings.basemapKind === 'pmtiles' && sources.pmtilesUrl !== undefined) {
      await ensurePmtilesProtocol(library);
    }
    return buildBasemapStyle(attrs.settings, sources, attrs.dark);
  }

  async function applyBasemap(): Promise<void> {
    const style = await resolveStyle();
    if (style !== undefined) map?.setStyle(style);
  }

  async function start(container: HTMLElement): Promise<void> {
    try {
      const [library] = await Promise.all([
        import('maplibre-gl'),
        import('maplibre-gl/dist/maplibre-gl.css'),
      ]);
      if (destroyed) return;
      lib = library;
      const style = await resolveStyle();
      if (destroyed || style === undefined) return;
      const settings = latest.settings;
      const created = new library.Map({
        container,
        style,
        center: INITIAL_CENTER,
        zoom: INITIAL_ZOOM,
        maxZoom: settings.maxZoom,
        attributionControl: {
          compact: false,
          ...(settings.attribution === '' ? {} : { customAttribution: settings.attribution }),
        },
      });
      map = created;
      created.addControl(new library.NavigationControl({ showCompass: false }), 'top-right');
      created.on('moveend', drawViewport);
      created.on('click', (event) => {
        popup?.remove();
        popup = undefined;
        if (latest.features !== undefined && latest.onFeatureClick !== undefined) {
          const hit = created
            .queryRenderedFeatures(event.point, {
              layers: FEATURE_LAYERS.filter((layer) => created.getLayer(layer) !== undefined),
            })
            .find((feature) => typeof feature.properties?._fmIndex === 'number');
          latest.onFeatureClick(hit === undefined ? undefined : Number(hit.properties._fmIndex));
        }
        latest.onMapClick?.({ latitude: event.lngLat.lat, longitude: event.lngLat.lng });
      });
      created.on('style.load', syncFeatures);
      created.once('load', () => {
        status = 'ready';
        syncFeatures();
        syncMarkers();
        appliedFitKey = latest.fitKey;
        if (!hasFitted) {
          hasFitted = true;
          fit();
        }
        m.redraw();
      });
    } catch {
      if (!destroyed) {
        status = 'failed';
        m.redraw();
      }
    }
  }

  return {
    oncreate: ({ dom }) => {
      const container = dom.querySelector<HTMLElement>('.fm-map-canvas');
      if (container !== null) void start(container);
    },
    onupdate: ({ attrs }) => {
      latest = attrs;
      if (map === undefined || status !== 'ready') return;
      if (basemapKey(attrs.settings, attrs.dark) !== appliedBasemap) void applyBasemap();
      syncFeatures();
      syncMarkers();
      applySelection();
      if (attrs.fitKey !== appliedFitKey) {
        appliedFitKey = attrs.fitKey;
        fit();
      }
    },
    onremove: () => {
      destroyed = true;
      clearDrawn();
      map?.remove();
      map = undefined;
      if (pmtilesProtocolRegistered) lib?.removeProtocol('pmtiles');
    },
    view: ({ attrs }) => {
      latest = attrs;
      const blank = !attrs.settings.onlineTiles && attrs.settings.basemapKind !== 'pmtiles';
      return m(
        '.fm-map',
        {
          role: 'region',
          'aria-label': attrs.ariaLabel ?? t('map', 'sectionTitle'),
          'data-theme': attrs.dark ? 'dark' : 'light',
          'data-basemap': blank ? 'blank' : 'imagery',
        },
        [
          m('.fm-map-canvas', { onbeforeupdate: () => false }),
          status === 'loading'
            ? m('.fm-map-status', { role: 'status' }, t('map', 'loading'))
            : undefined,
          status === 'failed'
            ? m('.fm-map-status.is-error', { role: 'alert' }, t('map', 'loadFailed'))
            : undefined,
          blank && status === 'ready' ? m('.fm-map-notice', t('map', 'blankGrid')) : undefined,
        ],
      );
    },
  };
};
