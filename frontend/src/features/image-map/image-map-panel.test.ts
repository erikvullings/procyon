import m from 'mithril';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { EntrySummary } from '../../models';
import { DEFAULT_MAP_SETTINGS } from '../map/map-settings';
import type { MapViewAttrs } from '../map/map-view';
import type { GeoImage } from './gps-scan';
import { ImageMapPanel, type ImageMapPanelAttrs, markersOf } from './image-map-panel';
import { type ImageMapState, INITIAL_IMAGE_MAP_STATE } from './image-map-state';

const captured: { attrs?: MapViewAttrs } = {};

vi.mock('../map/map-view', () => ({
  MapView: {
    view: ({ attrs }: { attrs: MapViewAttrs }) => {
      captured.attrs = attrs;
      return m('.stub-map');
    },
  },
}));

function entry(name: string): EntrySummary {
  return {
    id: name,
    name,
    kind: 'file',
    extension: name.split('.').pop(),
    location: { providerId: 'local', uri: `file:///${name}` },
  } as EntrySummary;
}

const geo = (name: string, latitude: number): GeoImage => ({
  entry: entry(name),
  latitude,
  longitude: 4,
  takenAt: '2024-05-05T10:00:00',
});

function attrs(
  state: ImageMapState,
  overrides: Partial<ImageMapPanelAttrs> = {},
): ImageMapPanelAttrs {
  return {
    state,
    client: {
      mapTileUrlTemplate: vi.fn(),
      mediaUrl: vi.fn(),
      getThumbnail: vi.fn().mockResolvedValue(undefined),
    },
    settings: DEFAULT_MAP_SETTINGS,
    dark: false,
    selectedIds: new Set(),
    onReveal: vi.fn(),
    onRecursive: vi.fn(),
    onCancel: vi.fn(),
    onBeginPlacing: vi.fn(),
    onStopPlacing: vi.fn(),
    onPropose: vi.fn(),
    onConfirm: vi.fn(),
    onCancelPlacement: vi.fn(),
    ...overrides,
  };
}

function render(panel: ImageMapPanelAttrs): HTMLElement {
  const root = document.createElement('div');
  document.body.append(root);
  m.render(root, m(ImageMapPanel, panel));
  return root;
}

afterEach(() => {
  document.body.replaceChildren();
});

const openState = (patch: Partial<ImageMapState>): ImageMapState => ({
  ...INITIAL_IMAGE_MAP_STATE,
  open: true,
  total: 3,
  ...patch,
});

describe('markersOf', () => {
  it('makes only writable formats draggable', () => {
    const markers = markersOf([geo('a.jpg', 1), geo('b.png', 2)]);
    expect(markers.map((marker) => marker.draggable)).toEqual([true, false]);
  });
});

describe('ImageMapPanel', () => {
  it('counts located and unlocated images and lists the unlocated separately', () => {
    const root = render(
      attrs(openState({ located: [geo('a.jpg', 1)], unlocated: [entry('b.jpg'), entry('c.png')] })),
    );
    expect(root.textContent).toContain('1 on the map, 2 without location');
    expect(root.querySelectorAll('.fm-image-map-row')).toHaveLength(2);
    // Only the JPEG can be placed.
    expect(root.querySelectorAll('.fm-image-map-place')).toHaveLength(1);
  });

  it('collapses the list of images without a location into a narrow strip and back', () => {
    const view = attrs(openState({ located: [geo('a.jpg', 1)], unlocated: [entry('b.jpg')] }));
    const root = render(view);
    const toggle = () => root.querySelector<HTMLButtonElement>('.fm-image-map-side-head button');
    expect(root.querySelector('.fm-image-map-side-rows')).not.toBeNull();
    toggle()?.click();
    m.render(root, m(ImageMapPanel, view));
    expect(root.querySelector('.fm-image-map-side.is-collapsed')).not.toBeNull();
    expect(root.querySelector('.fm-image-map-side-rows')).toBeNull();
    toggle()?.click();
    m.render(root, m(ImageMapPanel, view));
    expect(root.querySelector('.fm-image-map-side-rows')).not.toBeNull();
  });

  it('has no close button: the tab has its own', () => {
    const root = render(attrs(openState({ located: [geo('a.jpg', 1)] })));
    expect(root.querySelector('.fm-image-map-status')?.textContent).not.toContain('Close');
  });

  it('feeds the map markers, the selection highlight and the interaction callbacks', () => {
    const onReveal = vi.fn();
    const onPropose = vi.fn();
    render(
      attrs(openState({ located: [geo('a.jpg', 1), geo('b.jpg', 2)] }), {
        selectedIds: new Set(['a.jpg']),
        onReveal,
        onPropose,
      }),
    );
    const map = captured.attrs as MapViewAttrs;
    expect(map.markers.map((marker) => marker.id)).toEqual(['a.jpg', 'b.jpg']);
    expect(map.selectedIds).toEqual(new Set(['a.jpg']));
    const marker = map.markers[0];
    if (marker === undefined) throw new Error('marker missing');
    map.onMarkerClick?.(marker);
    expect(onReveal).toHaveBeenCalledWith(expect.objectContaining({ name: 'a.jpg' }));
    map.onMarkerDrag?.(marker, { latitude: 9, longitude: 8 });
    expect(onPropose).toHaveBeenCalledWith(expect.objectContaining({ name: 'a.jpg' }), 9, 8);
  });

  it('builds a popup with name, date and coordinates', () => {
    render(attrs(openState({ located: [geo('a.jpg', 52.5)] })));
    const map = captured.attrs as MapViewAttrs;
    const marker = map.markers[0];
    if (marker === undefined) throw new Error('marker missing');
    const popup = map.renderPopup?.(marker) as HTMLElement;
    expect(popup.textContent).toContain('a.jpg');
    expect(popup.textContent).toContain('Taken');
    expect(popup.textContent).toContain('52.50000, 4.00000');
  });

  it('places the image being positioned when the map is clicked', () => {
    const onPropose = vi.fn();
    const placing = entry('b.jpg');
    render(attrs(openState({ unlocated: [placing], placing }), { onPropose }));
    expect(document.body.textContent).toContain('Click the map to set the location of b.jpg');
    (captured.attrs as MapViewAttrs).onMapClick?.({ latitude: 5, longitude: 6 });
    expect(onPropose).toHaveBeenCalledWith(placing, 5, 6);
  });

  it('shows scan progress with a stop button and renders nothing when closed', () => {
    const onCancel = vi.fn();
    const scanning = render(attrs(openState({ scanning: true, done: 1, total: 3 }), { onCancel }));
    expect(scanning.textContent).toContain('1 of 3');
    [...scanning.querySelectorAll('button')].find((b) => b.textContent === 'Stop')?.click();
    expect(onCancel).toHaveBeenCalled();
    expect(render(attrs(INITIAL_IMAGE_MAP_STATE)).textContent).toBe('');
  });
});
