import type * as MapLibre from 'maplibre-gl';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { createDrawBridge, type DrawCallbacks } from './draw-bridge';

type Listener = (...args: unknown[]) => void;

const fake = vi.hoisted(() => {
  const state = {
    instances: [] as {
      listeners: Map<string, Listener>;
      calls: string[];
      features: Map<string, { geometry: unknown }>;
      mode: string;
      added: unknown[];
    }[],
  };
  class TerraDraw {
    listeners = new Map<string, Listener>();
    calls: string[] = [];
    features = new Map<string, { geometry: unknown }>();
    mode = 'static';
    added: unknown[] = [];
    constructor(public options: unknown) {
      state.instances.push(this);
    }
    on(name: string, listener: Listener): void {
      this.listeners.set(name, listener);
    }
    start(): void {
      this.calls.push('start');
    }
    stop(): void {
      this.calls.push('stop');
    }
    setMode(mode: string): void {
      this.mode = mode;
      this.calls.push(`mode:${mode}`);
    }
    getMode(): string {
      return this.mode;
    }
    clear(): void {
      this.calls.push('clear');
      // terra-draw reports removals through its change event while the layer is reloaded.
      this.listeners.get('change')?.(['x'], 'delete');
    }
    addFeatures(features: unknown[]): void {
      this.added = features;
      this.calls.push(`add:${features.length}`);
    }
    selectFeature(id: string): void {
      this.calls.push(`select:${id}`);
      this.listeners.get('select')?.(id);
    }
    deselectFeature(id: string): void {
      this.calls.push(`deselect:${id}`);
    }
    getSnapshot() {
      return [...this.features.keys()].map((id) => ({ id }));
    }
    getSnapshotFeature(id: string) {
      return this.features.get(id);
    }
  }
  class Mode {}
  return { state, TerraDraw, Mode };
});

vi.mock('terra-draw', () => ({
  TerraDraw: fake.TerraDraw,
  TerraDrawSelectMode: fake.Mode,
  TerraDrawPointMode: fake.Mode,
  TerraDrawLineStringMode: fake.Mode,
  TerraDrawPolygonMode: fake.Mode,
  TerraDrawRectangleMode: fake.Mode,
  TerraDrawCircleMode: fake.Mode,
}));
vi.mock('terra-draw-maplibre-gl-adapter', () => ({ TerraDrawMapLibreGLAdapter: class {} }));

function callbacks(): DrawCallbacks {
  return {
    onDrawn: vi.fn(),
    onChanged: vi.fn(),
    onDeleted: vi.fn(),
    onSelect: vi.fn(),
  };
}

const map = {} as MapLibre.Map;

beforeEach(() => {
  fake.state.instances.length = 0;
});

describe('createDrawBridge', () => {
  it('starts in select mode and switches tools', async () => {
    const bridge = await createDrawBridge(map, callbacks());
    const draw = fake.state.instances[0];
    expect(draw?.calls).toEqual(['start', 'mode:select']);
    bridge.setTool('polygon');
    expect(draw?.mode).toBe('polygon');
  });

  it('reports finished drawings and select-mode edits with rounded geometry', async () => {
    const handlers = callbacks();
    await createDrawBridge(map, handlers);
    const draw = fake.state.instances[0];
    draw?.features.set('f1', { geometry: { type: 'Point', coordinates: [4.123456789, 52] } });
    draw?.listeners.get('finish')?.('f1', { mode: 'point', action: 'draw' });
    expect(handlers.onDrawn).toHaveBeenCalledWith({ type: 'Point', coordinates: [4.1234568, 52] });
    draw?.listeners.get('finish')?.('f1', { mode: 'select', action: 'dragFeature' });
    expect(handlers.onChanged).toHaveBeenCalledWith('f1', {
      type: 'Point',
      coordinates: [4.1234568, 52],
    });
    draw?.features.set('f2', { geometry: { type: 'MultiPoint', coordinates: [] } });
    draw?.listeners.get('finish')?.('f2', { mode: 'point', action: 'draw' });
    expect(handlers.onDrawn).toHaveBeenCalledTimes(1);
  });

  it('reports selection, deselection and keyboard deletion', async () => {
    const handlers = callbacks();
    await createDrawBridge(map, handlers);
    const draw = fake.state.instances[0];
    draw?.listeners.get('select')?.('f1');
    expect(handlers.onSelect).toHaveBeenCalledWith('f1');
    draw?.listeners.get('deselect')?.('f1');
    expect(handlers.onSelect).toHaveBeenLastCalledWith(undefined);
    draw?.listeners.get('change')?.(['f1', 'f2'], 'delete');
    expect(handlers.onDeleted).toHaveBeenCalledWith(['f1', 'f2']);
  });

  it('reloads the layer from the document without echoing its own changes', async () => {
    const handlers = callbacks();
    const bridge = await createDrawBridge(map, handlers);
    const draw = fake.state.instances[0];
    bridge.load(
      [
        { id: 'a', mode: 'point', geometry: { type: 'Point', coordinates: [1, 2] } },
        { id: 'b', mode: 'point', geometry: { type: 'Point', coordinates: [3, 4] } },
      ],
      'b',
    );
    expect(draw?.calls.slice(2)).toEqual(['clear', 'add:2', 'select:b']);
    expect(draw?.added[0]).toMatchObject({ id: 'a', properties: { mode: 'point' } });
    expect(handlers.onDeleted).not.toHaveBeenCalled();
    expect(handlers.onSelect).not.toHaveBeenCalled();
  });

  it('does not select while a drawing tool is active and stops cleanly', async () => {
    const bridge = await createDrawBridge(map, callbacks());
    const draw = fake.state.instances[0];
    bridge.setTool('point');
    bridge.load([], 'a');
    expect(draw?.calls).not.toContain('select:a');
    bridge.destroy();
    bridge.destroy();
    expect(draw?.calls.filter((call) => call === 'stop')).toHaveLength(1);
    bridge.setTool('select');
    expect(draw?.mode).toBe('point');
  });
});
