import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { GeoDocument } from '../../models';
import type { MapViewAttrs } from '../map/map-view';
import type { DrawBridge, DrawCallbacks } from './draw-bridge';
import { GeoMapEditor, type GeoMapEditorAttrs } from './geo-map-editor';

const captured: { map?: MapViewAttrs; callbacks?: DrawCallbacks } = {};
const bridge: DrawBridge = {
  setTool: vi.fn(),
  load: vi.fn(),
  select: vi.fn(),
  destroy: vi.fn(),
};

vi.mock('../map/map-view', () => ({
  MapView: {
    view: ({ attrs }: { attrs: MapViewAttrs }) => {
      captured.map = attrs;
      return m('.stub-map');
    },
  },
}));
vi.mock('./draw-bridge', () => ({
  createDrawBridge: vi.fn(async (_map: unknown, callbacks: DrawCallbacks) => {
    captured.callbacks = callbacks;
    return bridge;
  }),
}));

const SAMPLE = JSON.stringify({
  type: 'FeatureCollection',
  features: [
    {
      type: 'Feature',
      geometry: { type: 'Point', coordinates: [4, 52] },
      properties: { name: 'A', n: 1 },
    },
    {
      type: 'Feature',
      geometry: { type: 'MultiPoint', coordinates: [[1, 1]] },
      properties: {},
    },
  ],
});

function parsed(error?: GeoDocument['error']): GeoDocument {
  return {
    format: 'geojson',
    collection: { type: 'FeatureCollection', features: [] },
    totalFeatures: 0,
    displayedFeatures: 0,
    truncated: false,
    warnings: [],
    skipped: [],
    ...(error === undefined ? {} : { error }),
  };
}

let root: HTMLElement;
let content: string;

function attrs(patch: Partial<GeoMapEditorAttrs> = {}): GeoMapEditorAttrs {
  return {
    client: {
      parseGeoText: vi.fn(async () => parsed()),
      getSettings: vi.fn().mockRejectedValue(new Error('offline')),
      mapTileUrlTemplate: vi.fn(),
      mediaUrl: vi.fn(),
    },
    content,
    onChange: (next) => {
      content = next;
    },
    ...patch,
  };
}

function mount(patch: Partial<GeoMapEditorAttrs> = {}): void {
  m.mount(root, { view: () => m(GeoMapEditor, attrs(patch)) });
  m.redraw.sync();
}

function button(label: string): HTMLButtonElement {
  const found = [...root.querySelectorAll('button')].find((item) => item.textContent === label);
  if (found === undefined) throw new Error(`button ${label} missing`);
  return found;
}

beforeEach(() => {
  root = document.createElement('div');
  document.body.append(root);
  content = SAMPLE;
  delete captured.map;
  delete captured.callbacks;
  vi.mocked(bridge.setTool).mockClear();
  vi.mocked(bridge.load).mockClear();
  vi.mocked(bridge.select).mockClear();
});

afterEach(() => {
  m.mount(root, null);
  document.body.replaceChildren();
});

async function ready(): Promise<void> {
  mount();
  captured.map?.onMapReady?.({} as never);
  await vi.waitFor(() => expect(captured.callbacks).toBeDefined());
  m.redraw.sync();
}

describe('GeoMapEditor', () => {
  it('lists the features and keeps multi-geometries read-only on the map', async () => {
    await ready();
    const items = [...root.querySelectorAll('[role="option"]')].map((item) => item.textContent);
    expect(items).toEqual(['Point 1 - A', 'MultiPoint 2']);
    expect(captured.map?.features?.features).toHaveLength(1);
    expect(root.textContent).toContain('edited in the JSON tab');
    // Only the editable point is handed to the drawing layer.
    expect(vi.mocked(bridge.load).mock.calls.at(-1)?.[0]).toHaveLength(1);
  });

  it('switches drawing tools', async () => {
    await ready();
    button('Polygon').click();
    m.redraw.sync();
    expect(bridge.setTool).toHaveBeenCalledWith('polygon');
    expect(button('Polygon').getAttribute('aria-pressed')).toBe('true');
  });

  it('adds a drawn shape to the file text and can undo it', async () => {
    await ready();
    captured.callbacks?.onDrawn({ type: 'Point', coordinates: [9, 9] });
    m.redraw.sync();
    expect(JSON.parse(content).features).toHaveLength(3);
    button('Undo').click();
    expect(JSON.parse(content).features).toHaveLength(2);
    m.redraw.sync();
    button('Redo').click();
    expect(JSON.parse(content).features).toHaveLength(3);
  });

  it('edits properties of the selected feature with typed values', async () => {
    await ready();
    root.querySelector<HTMLElement>('[role="option"]')?.click();
    m.redraw.sync();
    expect(bridge.select).toHaveBeenCalled();
    button('Properties').click();
    m.redraw.sync();
    const inputs = [
      ...root.querySelectorAll<HTMLInputElement>('.fm-geo-editor-properties tbody input'),
    ];
    // Rows: name/A, n/1 -> [key, value, key, value]
    const nValue = inputs[3];
    if (nValue === undefined) throw new Error('value input missing');
    nValue.value = '42';
    nValue.dispatchEvent(new Event('change'));
    expect(JSON.parse(content).features[0].properties.n).toBe(42);
    nValue.value = 'abc';
    nValue.dispatchEvent(new Event('change'));
    m.redraw.sync();
    expect(root.textContent).toContain('Enter a number.');
    expect(JSON.parse(content).features[0].properties.n).toBe(42);
  });

  it('moves the selected feature with Alt+Arrow keys', async () => {
    await ready();
    root.querySelector<HTMLElement>('[role="option"]')?.click();
    m.redraw.sync();
    root
      .querySelector('.fm-geo-editor')
      ?.dispatchEvent(
        new KeyboardEvent('keydown', { key: 'ArrowRight', altKey: true, bubbles: true }),
      );
    expect(JSON.parse(content).features[0].geometry.coordinates[0]).toBeCloseTo(4.00001, 6);
  });

  it('duplicates and deletes the selected feature', async () => {
    await ready();
    root.querySelector<HTMLElement>('[role="option"]')?.click();
    m.redraw.sync();
    button('Duplicate').click();
    expect(JSON.parse(content).features).toHaveLength(3);
    button('Delete').click();
    expect(JSON.parse(content).features).toHaveLength(2);
  });

  it('shows why a file cannot be edited on the map', () => {
    content = '{';
    mount();
    expect(root.querySelector('[role="alert"]')?.textContent).toContain('not valid GeoJSON');
    content = '{"type":"FeatureCollection","features":[],"pad":"' + 'x'.repeat(1_600_000) + '"}';
    m.redraw.sync();
    expect(root.querySelector('[role="alert"]')?.textContent).toContain('too large');
  });

  it('shows live validation of the JSON tab', async () => {
    const client = attrs().client;
    vi.mocked(client.parseGeoText).mockResolvedValue(
      parsed({ message: 'bad', line: 2, column: 3 }),
    );
    mount({ client });
    await vi.waitFor(() => {
      m.redraw.sync();
      button('JSON').click();
      m.redraw.sync();
      expect(root.textContent).toContain('Line 2, column 3: bad');
    });
  });
});
