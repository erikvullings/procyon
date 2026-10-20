import m from 'mithril';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { GeoDocument } from '../../models';
import type { MapViewAttrs } from '../map/map-view';
import { GeoView, type GeoViewAttrs } from './geo-view';

const captured: { attrs?: MapViewAttrs } = {};

vi.mock('../map/map-view', () => ({
  MapView: {
    view: ({ attrs }: { attrs: MapViewAttrs }) => {
      captured.attrs = attrs;
      return m('.stub-map');
    },
  },
}));

const document_: GeoDocument = {
  format: 'geojson',
  collection: {
    type: 'FeatureCollection',
    features: [
      {
        type: 'Feature',
        geometry: { type: 'Point', coordinates: [4, 52] },
        properties: { name: 'A', tags: ['x'], count: 3 },
      },
      { type: 'Feature', geometry: null, properties: {} },
    ],
  },
  totalFeatures: 5,
  displayedFeatures: 2,
  truncated: true,
  bounds: [4, 52, 4, 52],
  warnings: [{ path: '/crs', message: 'legacy crs' }],
  skipped: ['3D model'],
};

function attrs(patch: Partial<GeoViewAttrs> = {}): GeoViewAttrs {
  return {
    client: {
      mapTileUrlTemplate: vi.fn(),
      mediaUrl: vi.fn(),
      getSettings: vi.fn().mockRejectedValue(new Error('offline')),
    },
    name: 'places.geojson',
    document: document_,
    selectedIndex: undefined,
    onSelect: vi.fn(),
    onShowRaw: vi.fn(),
    ...patch,
  };
}

function render(viewAttrs: GeoViewAttrs): HTMLElement {
  const root = document.createElement('div');
  m.render(root, m(GeoView, viewAttrs));
  return root;
}

afterEach(() => {
  delete captured.attrs;
});

describe('GeoView', () => {
  it('passes the features and bounds to the map and states limits, warnings and skipped parts', () => {
    const root = render(attrs());
    expect(captured.attrs?.features).toBe(document_.collection);
    expect(captured.attrs?.dataBounds).toEqual([4, 52, 4, 52]);
    expect(root.textContent).toContain('5 features');
    expect(root.textContent).toContain('Only 2 of 5 features');
    expect(root.textContent).toContain('legacy crs');
    expect(root.textContent).toContain('3D model');
  });

  it('shows the property table of the selected feature and forwards map clicks', () => {
    const onSelect = vi.fn();
    const root = render(attrs({ selectedIndex: 0, onSelect }));
    const rows = [...root.querySelectorAll('.fm-geo-property')].map((row) => row.textContent);
    expect(rows).toEqual(['nameA', 'tags[x]', 'count3']);
    captured.attrs?.onFeatureClick?.(1);
    expect(onSelect).toHaveBeenCalledWith(1);
    const none = render(attrs({ selectedIndex: 1 }));
    expect(none.querySelector('[data-tooltip="Feature without geometry"]')).not.toBeNull();
    expect(none.textContent).toContain('no properties');
  });

  it('reports a read error with its position and offers the raw text', () => {
    const onShowRaw = vi.fn();
    const root = render(
      attrs({
        onShowRaw,
        document: {
          ...document_,
          collection: { type: 'FeatureCollection', features: [] },
          error: { message: 'bad json', line: 3, column: 9 },
        },
      }),
    );
    expect(root.textContent).toContain('bad json');
    expect(root.textContent).toContain('line 3, column 9');
    root.querySelector<HTMLButtonElement>('button')?.click();
    expect(onShowRaw).toHaveBeenCalled();
  });
});

describe('GeoView tracks (task 0279)', () => {
  const track = {
    name: 'Run',
    featureIndex: 0,
    points: 3,
    distanceMeters: 1500,
    elapsedSeconds: 600,
    movingSeconds: 540,
    ascentMeters: 20,
    descentMeters: 5,
    minElevation: 10,
    maxElevation: 30,
    averageSpeed: 2.8,
    maxSpeed: 4,
    startTime: '2024-05-05T08:00:00Z',
    endTime: '2024-05-05T08:10:00Z',
    profile: [
      { distanceMeters: 0, longitude: 4, latitude: 52, elevation: 10 },
      { distanceMeters: 750, longitude: 4.01, latitude: 52.01, elevation: 30 },
      { distanceMeters: 1500, longitude: 4.02, latitude: 52.02, elevation: 20 },
    ],
  };

  it('shows distance, time, climb and speed statistics', () => {
    const root = render(attrs({ document: { ...document_, tracks: [track] } }));
    const text = root.querySelector('.fm-track-summary')?.textContent ?? '';
    expect(text).toContain('Run');
    expect(text).toContain('1.50 km');
    expect(text).toContain('10:00');
    expect(text).toContain('9:00');
    expect(text).toContain('20 m');
    expect(text).toContain('10.1 km/h');
    expect(text).toContain('2024-05-05T08:00:00Z');
    expect(root.querySelector('.fm-elevation-chart')).not.toBeNull();
    expect(root.textContent).not.toContain('No reliable timestamps');
  });

  it('omits what the file cannot support and says so', () => {
    const root = render(
      attrs({
        document: {
          ...document_,
          tracks: [
            {
              ...track,
              elapsedSeconds: null,
              movingSeconds: null,
              averageSpeed: null,
              maxSpeed: null,
              ascentMeters: null,
              descentMeters: null,
              minElevation: null,
              maxElevation: null,
              profile: [{ distanceMeters: 0, longitude: 4, latitude: 52 }],
            },
          ],
        },
      }),
    );
    const text = root.querySelector('.fm-track-summary')?.textContent ?? '';
    expect(text).not.toContain('Elapsed time');
    expect(text).not.toContain('Ascent');
    expect(text).toContain('No reliable timestamps');
    expect(text).toContain('No elevation data');
    expect(root.querySelector('.fm-elevation-chart')).toBeNull();
  });

  it('shows the map position of the sample under the chart pointer', () => {
    const root = render(attrs({ document: { ...document_, tracks: [track] } }));
    const chart = root.querySelector('.fm-elevation-chart') as SVGElement;
    chart.getBoundingClientRect = () => ({ left: 0, width: 100 }) as DOMRect;
    chart.dispatchEvent(new PointerEvent('pointermove', { clientX: 50 }));
    m.render(root, m(GeoView, attrs({ document: { ...document_, tracks: [track] } })));
    expect(captured.attrs?.cursor).toEqual({ latitude: 52.01, longitude: 4.01 });
    chart.dispatchEvent(new PointerEvent('pointerleave'));
    m.render(root, m(GeoView, attrs({ document: { ...document_, tracks: [track] } })));
    expect(captured.attrs?.cursor).toBeUndefined();
  });
});

describe('GeoView KML (task 0280)', () => {
  const kml: GeoDocument = {
    format: 'kml',
    collection: {
      type: 'FeatureCollection',
      features: [
        {
          type: 'Feature',
          geometry: { type: 'Point', coordinates: [1, 1] },
          properties: {
            name: 'Museum',
            folder: 'Trip / Day 1',
            description:
              '<b>Open</b> <a href="http://evil.example">link</a><img src="http://evil.example/x.png"><script>alert(1)</script>',
          },
        },
        {
          type: 'Feature',
          geometry: { type: 'Point', coordinates: [2, 2] },
          properties: { name: 'Park', folder: 'Trip / Day 2' },
        },
      ],
    },
    totalFeatures: 2,
    displayedFeatures: 2,
    truncated: false,
    warnings: [],
    skipped: ['network link (not fetched)'],
    resources: { 'pin.png': 'data:image/png;base64,AA==' },
    overlays: [{ image: 'pin.png', north: 2, south: 1, east: 2, west: 1, rotation: 0 }],
  };

  it('lists folders as layers and hides the features of a switched-off folder', () => {
    const root = render(attrs({ document: kml }));
    const boxes = root.querySelectorAll<HTMLInputElement>('.fm-geo-layers input');
    expect(boxes).toHaveLength(3);
    expect(captured.attrs?.features?.features).toHaveLength(2);
    const day1 = [...boxes].find((box) => box.getAttribute('aria-label') === 'Show Day 1');
    if (day1 === undefined) throw new Error('Day 1 layer missing');
    day1.checked = false;
    day1.dispatchEvent(new Event('change'));
    m.render(root, m(GeoView, attrs({ document: kml })));
    expect(captured.attrs?.features?.features).toHaveLength(1);
    // A click on the remaining feature maps back to its original position in the file.
    const onSelect = vi.fn();
    m.render(root, m(GeoView, attrs({ document: kml, onSelect })));
    captured.attrs?.onFeatureClick?.(0);
    expect(onSelect).toHaveBeenCalledWith(1);
  });

  it('passes the shipped images and overlays to the map and states what was left out', () => {
    const root = render(attrs({ document: kml }));
    expect(captured.attrs?.icons).toEqual({ 'pin.png': 'data:image/png;base64,AA==' });
    expect(captured.attrs?.overlays).toEqual([
      expect.objectContaining({ id: '0', url: 'data:image/png;base64,AA==', north: 2 }),
    ]);
    expect(root.textContent).toContain('network link (not fetched)');
  });

  it('builds popups from sanitised descriptions: no scripts, images or live links', () => {
    render(attrs({ document: kml }));
    const popup = captured.attrs?.renderFeaturePopup?.(0) as HTMLElement;
    expect(popup.textContent).toContain('Museum');
    expect(popup.innerHTML).toContain('<b>Open</b>');
    expect(popup.innerHTML).not.toContain('script');
    expect(popup.innerHTML).not.toContain('<img');
    expect(popup.innerHTML).not.toContain('href');
    expect(captured.attrs?.renderFeaturePopup?.(1)).toBeDefined();
  });

  it('shows the description in the property rows as plain text', () => {
    const root = render(attrs({ document: kml, selectedIndex: 0 }));
    const row = [...root.querySelectorAll('.fm-geo-property')].find((item) =>
      item.textContent?.startsWith('description'),
    );
    expect(row?.textContent).toContain('Open');
    expect(row?.querySelector('b, a, img, script')).toBeNull();
  });
});

describe('GeoView feature list', () => {
  const many: GeoDocument = {
    ...document_,
    collection: {
      type: 'FeatureCollection',
      features: Array.from({ length: 1000 }, (_, index) => ({
        type: 'Feature' as const,
        geometry: { type: 'Point' as const, coordinates: [index / 100, 52] },
        properties: { name: `Place ${index}` },
      })),
    },
    totalFeatures: 1000,
    displayedFeatures: 1000,
    truncated: false,
  };

  it('lists every feature in one fixed-height row, rendering only the visible window', () => {
    const root = render(attrs({ document: many }));
    const rows = root.querySelectorAll('.fm-geo-feature');
    expect(rows.length).toBeGreaterThan(0);
    expect(rows.length).toBeLessThan(100);
    expect(root.querySelector<HTMLElement>('.fm-geo-rows')?.style.height).toBe('20000px');
    expect(rows[0]?.textContent).toBe('Place 0');
  });

  it('selects on click and unfolds the properties, with nested objects as a tree', () => {
    const nested: GeoDocument = {
      ...document_,
      collection: {
        type: 'FeatureCollection',
        features: [
          {
            type: 'Feature',
            geometry: {
              type: 'LineString',
              coordinates: [
                [0, 0],
                [1, 1],
              ],
            },
            properties: { name: 'Route', meta: { owner: 'me', depth: { level: 2 } } },
          },
        ],
      },
    };
    const onSelect = vi.fn();
    const root = render(attrs({ document: nested, onSelect }));
    root.querySelector<HTMLButtonElement>('.fm-geo-feature')?.click();
    expect(onSelect).toHaveBeenCalledWith(0);
    const view = attrs({ document: nested, onSelect, selectedIndex: 0 });
    m.render(root, m(GeoView, view));
    const texts = () =>
      [...root.querySelectorAll('.fm-geo-property')].map((row) => row.textContent);
    expect(texts()).toEqual(['nameRoute', '▸meta{2}']);
    root.querySelector<HTMLElement>('.fm-geo-property.is-nested')?.click();
    m.render(root, m(GeoView, view));
    expect(texts()).toEqual(['nameRoute', '▾meta{2}', 'ownerme', '▸depth{1}']);
  });

  it('collapses to the map only, with a property tooltip on hover, and maximises over the map', () => {
    const root = render(attrs());
    expect(captured.attrs?.renderFeatureHover).toBeUndefined();
    const buttons = () => [...root.querySelectorAll<HTMLButtonElement>('.fm-geo-panel-button')];
    buttons()[1]?.click();
    m.render(root, m(GeoView, attrs()));
    expect(root.querySelector('.fm-geo-feature')).toBeNull();
    const hover = captured.attrs?.renderFeatureHover?.(0) as HTMLElement;
    expect(hover.textContent).toContain('name');
    expect(hover.textContent).toContain('A');
    buttons()[0]?.click();
    m.render(root, m(GeoView, attrs()));
    expect(root.querySelector('.fm-geo-view')?.classList.contains('is-split')).toBe(true);
    buttons()[0]?.click();
    m.render(root, m(GeoView, attrs()));
    expect(root.querySelector('.fm-geo-view')?.classList.contains('is-full')).toBe(true);
  });
});
