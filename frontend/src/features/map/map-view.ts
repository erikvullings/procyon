import './map-view.css';
import type * as MapLibre from 'maplibre-gl';
import m, { type FactoryComponent } from 'mithril';
import { errorText, t } from '../../i18n';
import type { Location, MapSettings } from '../../models';
import { type BasemapSources, buildBasemapStyle } from './basemap-style';
import { type MapOverlay, overlayCorners } from './map-overlays';
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
  /**
   * Loads raster tiles straight from `settings.urlTemplate` instead of through the backend proxy,
   * which only knows the saved settings. For previewing settings that are not saved yet.
   */
  readonly directTiles?: boolean;
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
  /** Reference geometry drawn faintly beneath `features`, for before/after comparisons. */
  readonly comparison?: GeoJSON.FeatureCollection<GeoJSON.Geometry | null>;
  /** Extent of `features`; the map fits to it (when there are no markers) on load and fitKey changes. */
  readonly dataBounds?: readonly [number, number, number, number];
  /** Index (in `features.features`) of the feature that was clicked, or undefined for empty space. */
  readonly onFeatureClick?: (index: number | undefined) => void;
  /** Called once the map has loaded, so editors can attach their own layers (task 0278). */
  readonly onMapReady?: (map: MapLibre.Map) => void;
  /** Called when the map is torn down; attached layers are gone with it. */
  readonly onMapGone?: () => void;
  /** Images for point features that name one in `_icon`, as `data:` URIs by that name. */
  readonly icons?: Readonly<Record<string, string>>;
  /** Images draped over rectangles, drawn under the vector data. */
  readonly overlays?: readonly MapOverlay[];
  /** Content of the popup opened by clicking a feature; none when it returns undefined. */
  readonly renderFeaturePopup?: (index: number) => string | HTMLElement | undefined;
  /** Content of a tooltip shown while the pointer rests on a feature; none when it returns undefined. */
  readonly renderFeatureHover?: (index: number) => string | HTMLElement | undefined;
  /** A position to mark, for example under the pointer on an elevation chart. */
  readonly cursor?: MapPosition;
  /** Index of the feature to emphasise. */
  readonly highlightIndex?: number;
  readonly ariaLabel?: string;
}

type Status = 'loading' | 'ready' | 'failed';

/** How long the basemap may take to produce a usable style before the map reports a failure. */
const LOAD_TIMEOUT_MS = 20_000;

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
const COMPARE_SOURCE = 'fm-compare';
const FEATURE_LAYERS = [
  'fm-features-fill',
  'fm-features-line',
  'fm-features-circle',
  'fm-features-icon',
] as const;
const HIGHLIGHT_LAYER = 'fm-features-highlight';
const HIGHLIGHT_CASING_LAYER = 'fm-features-highlight-casing';
const HIGHLIGHT_POINT_LAYER = 'fm-features-highlight-point';
const HIGHLIGHT_COLOUR = 'rgb(255, 140, 0)';
const ICON_LAYER = 'fm-features-icon';
const OVERLAY_PREFIX = 'fm-overlay-';
const DEFAULT_FEATURE_COLOUR = 'rgb(25, 118, 210)';

/** `[west, south, east, north]` of a geometry, or undefined when it has no positions. */
export function geometryBounds(
  geometry: GeoJSON.Geometry,
): [number, number, number, number] | undefined {
  let bounds: [number, number, number, number] | undefined;
  const visit = (node: unknown): void => {
    if (!Array.isArray(node)) return;
    if (typeof node[0] === 'number' && typeof node[1] === 'number') {
      const [x, y] = node as [number, number];
      bounds = bounds
        ? [
            Math.min(bounds[0], x),
            Math.min(bounds[1], y),
            Math.max(bounds[2], x),
            Math.max(bounds[3], y),
          ]
        : [x, y, x, y];
      return;
    }
    for (const child of node) visit(child);
  };
  if (geometry.type === 'GeometryCollection') {
    for (const part of geometry.geometries) {
      const inner = geometryBounds(part);
      if (inner) {
        visit([inner[0], inner[1]]);
        visit([inner[2], inner[3]]);
      }
    }
  } else {
    visit(geometry.coordinates);
  }
  return bounds;
}

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

/** Short stable hash of the provider template, used to keep the web view's tile cache per provider. */
function templateVersion(template: string): string {
  let hash = 5381;
  for (const char of template) hash = (Math.imul(hash, 33) + (char.codePointAt(0) ?? 0)) >>> 0;
  return hash.toString(36);
}

function withVersion(tileTemplate: string, providerTemplate: string): string {
  const separator = tileTemplate.includes('?') ? '&' : '?';
  return `${tileTemplate}${separator}v=${templateVersion(providerTemplate)}`;
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
  /** Last error the map engine reported, shown with the failure message. */
  let failure: string | undefined;
  let destroyed = false;
  let lib: typeof MapLibre | undefined;
  let map: MapLibre.Map | undefined;
  let index: MarkerIndex | undefined;
  let indexedMarkers: readonly MapMarker[] | undefined;
  let appliedBasemap = '';
  let appliedFitKey: string | number | undefined;
  let hasFitted = false;
  let popup: MapLibre.Popup | undefined;
  let hoverPopup: MapLibre.Popup | undefined;
  let hoverIndex: number | undefined;
  /** Feature selected by a click on the map itself, which must not move the viewport. */
  let clickedIndex: number | undefined;
  const drawn = new Map<string, DrawnItem>();
  let cursorMarker: MapLibre.Marker | undefined;
  const iconLoads = new Set<string>();
  let pmtilesProtocolRegistered = false;

  async function resolveSources(attrs: MapViewAttrs): Promise<BasemapSources> {
    const { settings } = attrs;
    const tileTemplate =
      settings.basemapKind === 'raster' && settings.onlineTiles
        ? attrs.directTiles === true
          ? settings.urlTemplate
          : withVersion(await attrs.client.mapTileUrlTemplate(), settings.urlTemplate)
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

  function fitMap(): void {
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

  function syncCursor(): void {
    const library = lib;
    if (map === undefined || library === undefined) return;
    const position = latest.cursor;
    if (position === undefined) {
      cursorMarker?.remove();
      cursorMarker = undefined;
      return;
    }
    if (cursorMarker === undefined) {
      const element = document.createElement('div');
      element.className = 'fm-map-cursor';
      cursorMarker = new library.Marker({ element }).addTo(map);
    }
    cursorMarker.setLngLat([position.longitude, position.latitude]);
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
  let appliedComparison: GeoJSON.FeatureCollection<GeoJSON.Geometry | null> | undefined;
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
      if (target.getSource(COMPARE_SOURCE) === undefined) {
        target.addSource(COMPARE_SOURCE, {
          type: 'geojson',
          data: latest.comparison ?? { type: 'FeatureCollection', features: [] },
        } as never);
        appliedComparison = latest.comparison;
        target.addLayer({
          id: 'fm-compare-line',
          type: 'line',
          source: COMPARE_SOURCE,
          paint: { 'line-color': 'rgb(130, 130, 130)', 'line-width': 4, 'line-opacity': 0.5 },
        });
        target.addLayer({
          id: 'fm-compare-circle',
          type: 'circle',
          source: COMPARE_SOURCE,
          filter: ['==', ['geometry-type'], 'Point'],
          paint: {
            'circle-color': 'rgb(130, 130, 130)',
            'circle-radius': 7,
            'circle-opacity': 0.5,
          },
        });
      }
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
        filter: ['all', ['==', ['geometry-type'], 'Point'], ['!', ['has', '_icon']]],
        paint: {
          'circle-color': colour('marker-color') as never,
          'circle-radius': ['match', ['get', 'marker-size'], 'small', 4, 'large', 9, 6] as never,
          'circle-stroke-width': 1.5,
          'circle-stroke-color': 'rgb(255, 255, 255)',
        },
      });
      target.addLayer({
        id: ICON_LAYER,
        type: 'symbol',
        source: FEATURE_SOURCE,
        filter: ['all', ['==', ['geometry-type'], 'Point'], ['has', '_icon']],
        layout: {
          'icon-image': ['get', '_icon'] as never,
          'icon-allow-overlap': true,
          'icon-size': ['match', ['get', 'marker-size'], 'small', 0.6, 'large', 1.2, 1] as never,
        },
      });
      // A white casing under the coloured line keeps the selection visible on any background.
      target.addLayer({
        id: HIGHLIGHT_CASING_LAYER,
        type: 'line',
        source: FEATURE_SOURCE,
        filter: ['==', ['get', '_fmIndex'], -1],
        layout: { 'line-cap': 'round', 'line-join': 'round' },
        paint: { 'line-color': 'rgb(255, 255, 255)', 'line-width': 10, 'line-opacity': 0.9 },
      });
      target.addLayer({
        id: HIGHLIGHT_LAYER,
        type: 'line',
        source: FEATURE_SOURCE,
        filter: ['==', ['get', '_fmIndex'], -1],
        layout: { 'line-cap': 'round', 'line-join': 'round' },
        paint: { 'line-color': HIGHLIGHT_COLOUR, 'line-width': 6 },
      });
      target.addLayer({
        id: HIGHLIGHT_POINT_LAYER,
        type: 'circle',
        source: FEATURE_SOURCE,
        filter: ['==', ['get', '_fmIndex'], -1],
        paint: {
          'circle-radius': 14,
          'circle-color': HIGHLIGHT_COLOUR,
          'circle-opacity': 0.35,
          'circle-stroke-color': HIGHLIGHT_COLOUR,
          'circle-stroke-width': 4,
        },
      });
      appliedHighlight = undefined;
    }
    applyHighlight();
  }

  /** Registers the icon images; MapLibre forgets them with the style, so this runs again. */
  function ensureIcons(): void {
    const target = map;
    if (target === undefined) return;
    for (const [name, uri] of Object.entries(latest.icons ?? {})) {
      if (target.hasImage(name) || iconLoads.has(name)) continue;
      iconLoads.add(name);
      void target
        .loadImage(uri)
        .then((loadedImage) => {
          if (map === target && !target.hasImage(name)) target.addImage(name, loadedImage.data);
        })
        .catch(() => undefined)
        .finally(() => iconLoads.delete(name));
    }
  }

  /** Adds ground overlays under the vector data; safe to call again after a style swap. */
  function ensureOverlays(): void {
    const target = map;
    if (target === undefined) return;
    const wanted = new Set<string>();
    for (const overlay of latest.overlays ?? []) {
      const id = `${OVERLAY_PREFIX}${overlay.id}`;
      wanted.add(id);
      if (target.getSource(id) !== undefined) continue;
      target.addSource(id, {
        type: 'image',
        url: overlay.url,
        coordinates: overlayCorners(overlay),
      });
      target.addLayer(
        { id, type: 'raster', source: id, paint: { 'raster-opacity': 0.85 } },
        target.getLayer('fm-features-fill') === undefined ? undefined : 'fm-features-fill',
      );
    }
  }

  function applyHighlight(): void {
    if (map === undefined || map.getLayer(HIGHLIGHT_LAYER) === undefined) return;
    if (appliedHighlight === latest.highlightIndex) return;
    appliedHighlight = latest.highlightIndex;
    const wanted: MapLibre.FilterSpecification = [
      '==',
      ['get', '_fmIndex'],
      latest.highlightIndex ?? -1,
    ];
    map.setFilter(HIGHLIGHT_CASING_LAYER, wanted);
    map.setFilter(HIGHLIGHT_LAYER, wanted);
    map.setFilter(HIGHLIGHT_POINT_LAYER, ['all', wanted, ['==', ['geometry-type'], 'Point']]);
    focusHighlight();
  }

  /** Pans the highlighted feature to the centre of the map, keeping the zoom level. */
  function focusHighlight(): void {
    const index = latest.highlightIndex;
    if (index === clickedIndex) {
      clickedIndex = undefined;
      return;
    }
    const geometry = index === undefined ? undefined : latest.features?.features[index]?.geometry;
    const bounds = geometry == null ? undefined : geometryBounds(geometry);
    if (map === undefined || bounds === undefined) return;
    const [west, south, east, north] = bounds;
    map.easeTo({ center: [(west + east) / 2, (south + north) / 2] });
  }

  function hideHover(): void {
    hoverPopup?.remove();
    hoverPopup = undefined;
    hoverIndex = undefined;
  }

  function syncFeatures(): void {
    if (map === undefined) return;
    // Layers first, so that overlays can be inserted beneath them.
    ensureFeatureLayers();
    ensureOverlays();
    ensureIcons();
    if (appliedFeatures !== latest.features) {
      appliedFeatures = latest.features;
      const source = map.getSource(FEATURE_SOURCE) as MapLibre.GeoJSONSource | undefined;
      source?.setData(
        withFeatureIndexes(latest.features ?? { type: 'FeatureCollection', features: [] }),
      );
    }
    if (appliedComparison !== latest.comparison) {
      appliedComparison = latest.comparison;
      const compare = map.getSource(COMPARE_SOURCE) as MapLibre.GeoJSONSource | undefined;
      compare?.setData((latest.comparison ?? { type: 'FeatureCollection', features: [] }) as never);
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

  /** MapLibre opens a compact attribution control at first; this folds it back to its icon. */
  function minimizeAttribution(container: HTMLElement): void {
    container
      .querySelector('.maplibregl-ctrl-attrib.maplibregl-compact')
      ?.classList.remove('maplibregl-compact-show');
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
        // The credit line is a small icon until clicked: it covers a lot of the map otherwise.
        attributionControl: {
          compact: true,
          ...(settings.attribution === '' ? {} : { customAttribution: settings.attribution }),
        },
      });
      map = created;
      created.addControl(new library.NavigationControl({ showCompass: false }), 'top-right');
      created.on('moveend', drawViewport);
      created.on('click', (event) => {
        popup?.remove();
        popup = undefined;
        if (
          latest.features !== undefined &&
          (latest.onFeatureClick !== undefined || latest.renderFeaturePopup !== undefined)
        ) {
          const hit = created
            .queryRenderedFeatures(event.point, {
              layers: FEATURE_LAYERS.filter((layer) => created.getLayer(layer) !== undefined),
            })
            .find((feature) => typeof feature.properties?._fmIndex === 'number');
          const index = hit === undefined ? undefined : Number(hit.properties._fmIndex);
          hideHover();
          clickedIndex = index;
          latest.onFeatureClick?.(index);
          const content = index === undefined ? undefined : latest.renderFeaturePopup?.(index);
          if (content !== undefined) {
            const next = new library.Popup({ offset: 8, closeButton: true });
            next.setLngLat(event.lngLat);
            if (typeof content === 'string') next.setText(content);
            else next.setDOMContent(content);
            next.addTo(created);
            popup = next;
          }
        }
        latest.onMapClick?.({ latitude: event.lngLat.lat, longitude: event.lngLat.lng });
      });
      created.on('mousemove', (event) => {
        const render = latest.renderFeatureHover;
        if (render === undefined || latest.features === undefined) return hideHover();
        const hit = created
          .queryRenderedFeatures(event.point, {
            layers: FEATURE_LAYERS.filter((layer) => created.getLayer(layer) !== undefined),
          })
          .find((feature) => typeof feature.properties?._fmIndex === 'number');
        const index = hit === undefined ? undefined : Number(hit.properties._fmIndex);
        if (index === undefined) return hideHover();
        if (index === hoverIndex && hoverPopup !== undefined) {
          hoverPopup.setLngLat(event.lngLat);
          return;
        }
        hideHover();
        const content = render(index);
        if (content === undefined) return;
        hoverIndex = index;
        const next = new library.Popup({
          offset: 12,
          closeButton: false,
          closeOnClick: false,
          className: 'fm-geo-hover',
          maxWidth: '24rem',
        });
        next.setLngLat(event.lngLat);
        if (typeof content === 'string') next.setText(content);
        else next.setDOMContent(content);
        next.addTo(created);
        hoverPopup = next;
      });
      created.on('mouseout', hideHover);
      created.on('style.load', syncFeatures);
      // 'load' also waits for the first tiles, which may never arrive; the style is enough to draw.
      const loadTimer = setTimeout(() => {
        if (status === 'loading' && !destroyed) {
          failure = failure ?? t('map', 'loadTimeout');
          status = 'failed';
          m.redraw();
        }
      }, LOAD_TIMEOUT_MS);
      created.on('error', (event) => {
        failure = event.error instanceof Error ? event.error.message : String(event.error);
        console.error('map error', event.error);
        if (status === 'loading' && !created.isStyleLoaded()) {
          status = 'failed';
          m.redraw();
        }
      });
      let started = false;
      const becomeReady = (): void => {
        if (started || destroyed) return;
        started = true;
        clearTimeout(loadTimer);
        status = 'ready';
        syncFeatures();
        minimizeAttribution(container);
        created.once('idle', () => minimizeAttribution(container));
        latest.onMapReady?.(created);
        syncMarkers();
        appliedFitKey = latest.fitKey;
        if (!hasFitted) {
          hasFitted = true;
          fitMap();
        }
        m.redraw();
      };
      created.once('style.load', becomeReady);
      created.once('load', becomeReady);
    } catch (error) {
      console.error('map failed to start', error);
      if (!destroyed) {
        failure = errorText(error);
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
      syncCursor();
      applySelection();
      if (attrs.fitKey !== appliedFitKey) {
        appliedFitKey = attrs.fitKey;
        fitMap();
      }
    },
    onremove: () => {
      destroyed = true;
      latest.onMapGone?.();
      clearDrawn();
      cursorMarker?.remove();
      cursorMarker = undefined;
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
            ? m(
                '.fm-map-status.is-error',
                { role: 'alert' },
                failure === undefined
                  ? t('map', 'loadFailed')
                  : `${t('map', 'loadFailed')} ${failure}`,
              )
            : undefined,
          blank && status === 'ready' ? m('.fm-map-notice', t('map', 'blankGrid')) : undefined,
        ],
      );
    },
  };
};
