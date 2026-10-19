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
    expect(root.textContent).toContain('5 feature(s)');
    expect(root.textContent).toContain('Only 2 of 5 features');
    expect(root.textContent).toContain('legacy crs');
    expect(root.textContent).toContain('3D model');
    expect(root.textContent).toContain('Click a feature');
  });

  it('shows the property table of the selected feature and forwards map clicks', () => {
    const onSelect = vi.fn();
    const root = render(attrs({ selectedIndex: 0, onSelect }));
    const rows = [...root.querySelectorAll('tr')].map((row) => row.textContent);
    expect(rows).toEqual(['nameA', 'tags["x"]', 'count3']);
    captured.attrs?.onFeatureClick?.(1);
    expect(onSelect).toHaveBeenCalledWith(1);
    const none = render(attrs({ selectedIndex: 1 }));
    expect(none.textContent).toContain('Feature without geometry');
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
    const chart = root.querySelector('svg') as SVGElement;
    chart.getBoundingClientRect = () => ({ left: 0, width: 100 }) as DOMRect;
    chart.dispatchEvent(new PointerEvent('pointermove', { clientX: 50 }));
    m.render(root, m(GeoView, attrs({ document: { ...document_, tracks: [track] } })));
    expect(captured.attrs?.cursor).toEqual({ latitude: 52.01, longitude: 4.01 });
    chart.dispatchEvent(new PointerEvent('pointerleave'));
    m.render(root, m(GeoView, attrs({ document: { ...document_, tracks: [track] } })));
    expect(captured.attrs?.cursor).toBeUndefined();
  });
});
