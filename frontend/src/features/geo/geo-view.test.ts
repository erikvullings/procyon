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
