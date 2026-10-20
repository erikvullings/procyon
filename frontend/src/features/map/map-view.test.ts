import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { MapSettings } from '../../models';
import { DEFAULT_MAP_SETTINGS } from './map-settings';
import { geometryBounds, MapView, type MapViewAttrs, type MapViewClient } from './map-view';
import type { MapMarker } from './marker-index';

const fake = vi.hoisted(() => {
  class Evented {
    handlers = new Map<string, ((event?: unknown) => void)[]>();
    on(name: string, handler: (event?: unknown) => void): this {
      this.handlers.set(name, [...(this.handlers.get(name) ?? []), handler]);
      return this;
    }
    once(name: string, handler: (event?: unknown) => void): this {
      return this.on(name, handler);
    }
    fire(name: string, event?: unknown): void {
      for (const handler of this.handlers.get(name) ?? []) handler(event);
    }
  }
  class FakeMap extends Evented {
    removed = false;
    sources = new Map<string, { data: unknown; setData(data: unknown): void }>();
    layers = new Map<string, Record<string, unknown>>();
    images = new Set<string>();
    loadedImages: string[] = [];
    renderedAt: unknown[] = [];
    calls: { method: string; args: unknown[] }[] = [];
    controls: unknown[] = [];
    constructor(public options: Record<string, unknown>) {
      super();
      maps.push(this);
    }
    addControl(control: unknown): void {
      this.controls.push(control);
    }
    hasImage(name: string): boolean {
      return this.images.has(name);
    }
    async loadImage(url: string) {
      this.loadedImages.push(url);
      return { data: { width: 1, height: 1 } };
    }
    addImage(name: string): void {
      this.images.add(name);
    }
    getSource(id: string) {
      return this.sources.get(id);
    }
    addSource(id: string, spec: { data?: unknown }): void {
      const source = {
        data: spec.data,
        setData(data: unknown) {
          source.data = data;
        },
      };
      this.sources.set(id, source);
    }
    getLayer(id: string) {
      return this.layers.get(id);
    }
    addLayer(layer: { id: string }, before?: string): void {
      this.layers.set(layer.id, { ...layer, before });
    }
    setFilter(id: string, filter: unknown): void {
      this.calls.push({ method: 'setFilter', args: [id, filter] });
    }
    queryRenderedFeatures() {
      return this.renderedAt;
    }
    getBounds() {
      return { getWest: () => -180, getSouth: () => -85, getEast: () => 180, getNorth: () => 85 };
    }
    getZoom(): number {
      return zoom.value;
    }
    easeTo(...args: unknown[]): void {
      this.calls.push({ method: 'easeTo', args });
    }
    jumpTo(...args: unknown[]): void {
      this.calls.push({ method: 'jumpTo', args });
    }
    fitBounds(...args: unknown[]): void {
      this.calls.push({ method: 'fitBounds', args });
    }
    setStyle(...args: unknown[]): void {
      this.calls.push({ method: 'setStyle', args });
    }
    remove(): void {
      this.removed = true;
    }
  }
  class FakeMarker extends Evented {
    removed = false;
    lngLat: [number, number] = [0, 0];
    constructor(public options: { element: HTMLElement; draggable?: boolean }) {
      super();
      markers.push(this);
    }
    setLngLat(lngLat: [number, number]): this {
      this.lngLat = lngLat;
      return this;
    }
    getLngLat() {
      return { lng: this.lngLat[0], lat: this.lngLat[1] };
    }
    addTo(): this {
      document.body.append(this.options.element);
      return this;
    }
    remove(): void {
      this.removed = true;
      this.options.element.remove();
    }
  }
  class FakePopup {
    content: unknown;
    removed = false;
    constructor() {
      popups.push(this);
    }
    setLngLat(): this {
      return this;
    }
    setText(text: string): this {
      this.content = text;
      return this;
    }
    setDOMContent(node: Node): this {
      this.content = node;
      return this;
    }
    addTo(): this {
      return this;
    }
    remove(): void {
      this.removed = true;
    }
  }
  const maps: FakeMap[] = [];
  const markers: FakeMarker[] = [];
  const popups: FakePopup[] = [];
  const zoom = { value: 2 };
  const protocols: string[] = [];
  return { FakeMap, FakeMarker, FakePopup, maps, markers, popups, zoom, protocols };
});

vi.mock('maplibre-gl', () => ({
  Map: fake.FakeMap,
  Marker: fake.FakeMarker,
  Popup: fake.FakePopup,
  NavigationControl: class {},
  addProtocol: (name: string) => fake.protocols.push(name),
  removeProtocol: (name: string) => fake.protocols.splice(fake.protocols.indexOf(name), 1),
}));
vi.mock('maplibre-gl/dist/maplibre-gl.css', () => ({}));
vi.mock('pmtiles', () => ({
  Protocol: class {
    tile = () => undefined;
  },
}));

const markers: MapMarker[] = [
  { id: 'a', latitude: 52, longitude: 4, title: 'A' },
  { id: 'b', latitude: 52.0001, longitude: 4.0001, title: 'B' },
  { id: 'c', latitude: -33.9, longitude: 151.2, title: 'C' },
];

function client(): MapViewClient & {
  mapTileUrlTemplate: ReturnType<typeof vi.fn>;
  mediaUrl: ReturnType<typeof vi.fn>;
} {
  return {
    mapTileUrlTemplate: vi.fn().mockResolvedValue('/api/v1/map/tiles/{z}/{x}/{y}'),
    mediaUrl: vi.fn().mockResolvedValue('/api/v1/media?location=x'),
  };
}

let root: HTMLElement;

function mount(attrs: () => MapViewAttrs): void {
  m.mount(root, { view: () => m(MapView, attrs()) });
}

async function loaded(): Promise<InstanceType<typeof fake.FakeMap>> {
  await vi.waitFor(() => expect(fake.maps).toHaveLength(1));
  const map = fake.maps[0];
  if (map === undefined) throw new Error('map missing');
  map.fire('load');
  m.redraw.sync();
  return map;
}

beforeEach(() => {
  root = document.createElement('div');
  document.body.append(root);
  fake.maps.length = 0;
  fake.markers.length = 0;
  fake.popups.length = 0;
  fake.zoom.value = 2;
});

afterEach(() => {
  m.mount(root, null);
  document.body.replaceChildren();
});

describe('MapView', () => {
  it('requests nothing until it is mounted and then builds the proxy-backed basemap', async () => {
    const api = client();
    expect(api.mapTileUrlTemplate).not.toHaveBeenCalled();
    mount(() => ({ client: api, settings: DEFAULT_MAP_SETTINGS, dark: false, markers: [] }));
    const map = await loaded();
    expect(api.mapTileUrlTemplate).toHaveBeenCalledTimes(1);
    expect(map.options.style).toMatchObject({
      sources: {
        basemap: {
          tiles: [expect.stringMatching(/^\/api\/v1\/map\/tiles\/\{z\}\/\{x\}\/\{y\}\?v=\w+$/)],
        },
      },
    });
    expect(map.options.attributionControl).toMatchObject({
      compact: true,
      customAttribution: DEFAULT_MAP_SETTINGS.attribution,
    });
  });

  it('loads the draft template directly when previewing unsaved settings', async () => {
    const api = client();
    mount(() => ({
      client: api,
      settings: DEFAULT_MAP_SETTINGS,
      directTiles: true,
      dark: false,
      markers: [],
    }));
    const map = await loaded();
    expect(api.mapTileUrlTemplate).not.toHaveBeenCalled();
    expect(map.options.style).toMatchObject({
      sources: { basemap: { tiles: [DEFAULT_MAP_SETTINGS.urlTemplate] } },
    });
  });

  it('never asks the client for tiles when online tiles are disabled', async () => {
    const api = client();
    const offline: MapSettings = { ...DEFAULT_MAP_SETTINGS, onlineTiles: false };
    mount(() => ({ client: api, settings: offline, dark: true, markers: [] }));
    const map = await loaded();
    expect(api.mapTileUrlTemplate).not.toHaveBeenCalled();
    expect(JSON.stringify(map.options.style)).not.toMatch(/https?:\/\//);
    expect(root.textContent).toContain('blank grid');
  });

  it('reads a local PMTiles file through the media URL and registers the protocol', async () => {
    const api = client();
    const settings: MapSettings = {
      ...DEFAULT_MAP_SETTINGS,
      basemapKind: 'pmtiles',
      pmtilesLocation: { providerId: 'local', uri: 'file:///maps/w.pmtiles' },
    };
    mount(() => ({ client: api, settings, dark: false, markers: [] }));
    await loaded();
    expect(api.mediaUrl).toHaveBeenCalledWith({
      providerId: 'local',
      uri: 'file:///maps/w.pmtiles',
    });
    expect(fake.protocols).toEqual(['pmtiles']);
    m.mount(root, null);
    expect(fake.protocols).toEqual([]);
  });

  it('clusters markers, zooms into a cluster on click and fits to the markers once', async () => {
    mount(() => ({ client: client(), settings: DEFAULT_MAP_SETTINGS, dark: false, markers }));
    const map = await loaded();
    expect(map.calls.filter((call) => call.method === 'fitBounds')).toHaveLength(1);
    const clusters = [...document.querySelectorAll<HTMLElement>('.fm-map-cluster')];
    expect(clusters.map((element) => element.textContent)).toEqual(['2']);
    expect(document.querySelectorAll('.fm-map-marker')).toHaveLength(1);
    clusters[0]?.click();
    expect(map.calls.find((call) => call.method === 'easeTo')).toBeDefined();
  });

  it('opens a popup and reports clicks on a single marker', async () => {
    const onMarkerClick = vi.fn();
    fake.zoom.value = 18;
    mount(() => ({
      client: client(),
      settings: DEFAULT_MAP_SETTINGS,
      dark: false,
      markers,
      renderPopup: (marker) => `Popup ${marker.title}`,
      onMarkerClick,
    }));
    await loaded();
    const first = document.querySelector<HTMLElement>('.fm-map-marker');
    first?.click();
    expect(onMarkerClick).toHaveBeenCalledTimes(1);
    expect(fake.popups.at(-1)?.content).toMatch(/^Popup /);
  });

  it('highlights selected markers without redrawing them and reports drags', async () => {
    const onMarkerDrag = vi.fn();
    fake.zoom.value = 18;
    let selected = new Set<string>();
    const draggable = markers.map((marker) => ({ ...marker, draggable: true }));
    mount(() => ({
      client: client(),
      settings: DEFAULT_MAP_SETTINGS,
      dark: false,
      markers: draggable,
      selectedIds: selected,
      onMarkerDrag,
    }));
    await loaded();
    expect(document.querySelectorAll('.fm-map-marker.is-selected')).toHaveLength(0);
    const before = fake.markers.length;
    selected = new Set(['a']);
    m.redraw.sync();
    expect(document.querySelectorAll('.fm-map-marker.is-selected')).toHaveLength(1);
    expect(fake.markers).toHaveLength(before);

    const dragged = fake.markers.find((entry) => entry.options.draggable === true);
    dragged?.setLngLat([5, 6]);
    dragged?.fire('dragend');
    expect(onMarkerDrag).toHaveBeenCalledWith(expect.objectContaining({ draggable: true }), {
      latitude: 6,
      longitude: 5,
    });
  });

  it('swaps the style when the basemap settings change and removes the map on teardown', async () => {
    let settings: MapSettings = DEFAULT_MAP_SETTINGS;
    mount(() => ({ client: client(), settings, dark: false, markers: [] }));
    const map = await loaded();
    settings = { ...DEFAULT_MAP_SETTINGS, onlineTiles: false };
    m.redraw.sync();
    await vi.waitFor(() => expect(map.calls.some((call) => call.method === 'setStyle')).toBe(true));
    m.mount(root, null);
    expect(map.removed).toBe(true);
  });

  it('reports a failure to load the map', async () => {
    const api = client();
    api.mapTileUrlTemplate.mockRejectedValue(new Error('offline'));
    mount(() => ({ client: api, settings: DEFAULT_MAP_SETTINGS, dark: false, markers: [] }));
    await vi.waitFor(() => {
      m.redraw.sync();
      expect(root.querySelector('[role="alert"]')).not.toBeNull();
    });
  });

  it('draws vector features with simplestyle expressions, fits to their bounds and maps clicks back to a feature', async () => {
    const onFeatureClick = vi.fn();
    const collection = {
      type: 'FeatureCollection' as const,
      features: [
        {
          type: 'Feature' as const,
          geometry: { type: 'Point' as const, coordinates: [4, 52] },
          properties: { 'marker-color': '#ff0000' },
        },
        {
          type: 'Feature' as const,
          geometry: {
            type: 'LineString' as const,
            coordinates: [
              [0, 0],
              [1, 1],
            ],
          },
          properties: null,
        },
      ],
    };
    let highlightIndex: number | undefined;
    mount(() => ({
      client: client(),
      settings: DEFAULT_MAP_SETTINGS,
      dark: false,
      markers: [],
      features: collection,
      dataBounds: [0, 0, 4, 52],
      onFeatureClick,
      ...(highlightIndex === undefined ? {} : { highlightIndex }),
    }));
    const map = await loaded();
    expect([...map.layers.keys()]).toEqual([
      'fm-compare-line',
      'fm-compare-circle',
      'fm-features-fill',
      'fm-features-line',
      'fm-features-circle',
      'fm-features-icon',
      'fm-features-highlight-casing',
      'fm-features-highlight',
      'fm-features-highlight-point',
    ]);
    const source = map.sources.get('fm-features');
    expect(JSON.stringify(source?.data)).toContain('"_fmIndex":1');
    expect(JSON.stringify(map.layers.get('fm-features-circle'))).toContain('marker-color');
    expect(map.calls.find((call) => call.method === 'fitBounds')?.args[0]).toEqual([
      [0, 0],
      [4, 52],
    ]);

    map.renderedAt = [{ properties: { _fmIndex: 1 } }];
    map.fire('click', { lngLat: { lat: 0, lng: 0 }, point: { x: 1, y: 1 } });
    expect(onFeatureClick).toHaveBeenLastCalledWith(1);
    map.renderedAt = [];
    map.fire('click', { lngLat: { lat: 0, lng: 0 }, point: { x: 1, y: 1 } });
    expect(onFeatureClick).toHaveBeenLastCalledWith(undefined);

    highlightIndex = 1;
    m.redraw.sync();
    expect(
      map.calls.findLast(
        (call) => call.method === 'setFilter' && call.args[0] === 'fm-features-highlight',
      )?.args[1],
    ).toEqual(['==', ['get', '_fmIndex'], 1]);
  });

  it('shows and moves a cursor marker, and removes it when the cursor goes away', async () => {
    let cursor: { latitude: number; longitude: number } | undefined = { latitude: 1, longitude: 2 };
    mount(() => ({
      client: client(),
      settings: DEFAULT_MAP_SETTINGS,
      dark: false,
      markers: [],
      ...(cursor === undefined ? {} : { cursor }),
    }));
    await loaded();
    m.redraw.sync();
    const cursorMarker = fake.markers.find((entry) =>
      entry.options.element.classList.contains('fm-map-cursor'),
    );
    expect(cursorMarker?.lngLat).toEqual([2, 1]);
    cursor = { latitude: 5, longitude: 6 };
    m.redraw.sync();
    expect(cursorMarker?.lngLat).toEqual([6, 5]);
    cursor = undefined;
    m.redraw.sync();
    expect(cursorMarker?.removed).toBe(true);
  });

  it('registers icons, drapes overlays under the vector data and opens a popup on a feature click', async () => {
    const collection = {
      type: 'FeatureCollection' as const,
      features: [
        {
          type: 'Feature' as const,
          geometry: { type: 'Point' as const, coordinates: [4, 52] },
          properties: { _icon: 'pin.png', name: 'Pin' },
        },
      ],
    };
    mount(() => ({
      client: client(),
      settings: DEFAULT_MAP_SETTINGS,
      dark: false,
      markers: [],
      features: collection,
      icons: { 'pin.png': 'data:image/png;base64,AA==' },
      overlays: [
        { id: '0', url: 'data:image/png;base64,AA==', north: 2, south: 1, east: 2, west: 1 },
      ],
      renderFeaturePopup: (index) => `Feature ${index}`,
    }));
    const map = await loaded();
    await vi.waitFor(() => expect(map.images.has('pin.png')).toBe(true));
    expect(map.loadedImages).toEqual(['data:image/png;base64,AA==']);
    const overlay = map.layers.get('fm-overlay-0');
    expect(overlay).toMatchObject({ type: 'raster', before: 'fm-features-fill' });
    expect(JSON.stringify(map.layers.get('fm-features-icon'))).toContain('_icon');

    map.renderedAt = [{ properties: { _fmIndex: 0 } }];
    map.fire('click', { lngLat: { lat: 52, lng: 4 }, point: { x: 1, y: 1 } });
    expect(fake.popups.at(-1)?.content).toBe('Feature 0');
  });
});

describe('geometryBounds', () => {
  it('spans points, lines and nested polygons', () => {
    expect(geometryBounds({ type: 'Point', coordinates: [4, 52] })).toEqual([4, 52, 4, 52]);
    expect(
      geometryBounds({
        type: 'LineString',
        coordinates: [
          [4, 52],
          [5, 51],
        ],
      }),
    ).toEqual([4, 51, 5, 52]);
    expect(
      geometryBounds({
        type: 'MultiPolygon',
        coordinates: [
          [
            [
              [0, 0],
              [2, 0],
              [2, 3],
              [0, 0],
            ],
          ],
        ],
      }),
    ).toEqual([0, 0, 2, 3]);
    expect(geometryBounds({ type: 'GeometryCollection', geometries: [] })).toBeUndefined();
  });
});
