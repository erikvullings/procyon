import type * as MapLibre from 'maplibre-gl';
import type { EditableDrawFeature } from './draw-features';
import { fromDrawGeometry } from './draw-features';
import { type DrawGeometry, newEditorId } from './geo-edit-model';
import type { DrawTool } from './geo-edit-session';

/** What the drawing layer reports back to the editor session. */
export interface DrawCallbacks {
  /** A new shape was finished by the user. */
  onDrawn(geometry: DrawGeometry): void;
  /** A selected shape or one of its vertices was moved, inserted or deleted. */
  onChanged(id: string, geometry: DrawGeometry): void;
  /** A selected shape was deleted with the keyboard. */
  onDeleted(ids: readonly string[]): void;
  onSelect(id: string | undefined): void;
}

export interface DrawBridge {
  setTool(tool: DrawTool): void;
  /** Replaces everything on the drawing layer with the editable features of the document. */
  load(features: readonly EditableDrawFeature[], selectedId: string | undefined): void;
  select(id: string | undefined): void;
  destroy(): void;
}

const SELECT_FLAGS = {
  feature: {
    draggable: true,
    coordinates: { midpoints: true, draggable: true, deletable: true },
  },
} as const;

/**
 * Connects the terra-draw editing library to a MapLibre map. terra-draw handles pointer and
 * keyboard interaction (draw, select, drag, add or delete vertices); the editor session stays the
 * source of truth, so after every change the layer is reloaded from the document. The library is
 * imported on first use so that viewing a map does not pay for editing code.
 */
export async function createDrawBridge(
  map: MapLibre.Map,
  callbacks: DrawCallbacks,
): Promise<DrawBridge> {
  const [terra, adapter] = await Promise.all([
    import('terra-draw'),
    import('terra-draw-maplibre-gl-adapter'),
  ]);
  const draw = new terra.TerraDraw({
    adapter: new adapter.TerraDrawMapLibreGLAdapter({ map }),
    idStrategy: {
      isValidId: (id) => typeof id === 'string' && id.length > 0,
      getId: () => newEditorId(),
    },
    modes: [
      new terra.TerraDrawSelectMode({
        flags: {
          point: { feature: { draggable: true } },
          linestring: SELECT_FLAGS,
          polygon: SELECT_FLAGS,
          rectangle: SELECT_FLAGS,
          circle: SELECT_FLAGS,
        },
      }),
      new terra.TerraDrawPointMode(),
      new terra.TerraDrawLineStringMode(),
      new terra.TerraDrawPolygonMode(),
      new terra.TerraDrawRectangleMode(),
      new terra.TerraDrawCircleMode(),
    ],
  });
  let loading = 0;
  let destroyed = false;

  draw.on('finish', (id, context) => {
    if (loading > 0 || destroyed) return;
    const geometry = fromDrawGeometry(draw.getSnapshotFeature(id)?.geometry);
    if (geometry === undefined) return;
    if (context.mode === 'select') callbacks.onChanged(String(id), geometry);
    else callbacks.onDrawn(geometry);
  });
  draw.on('select', (id) => {
    if (!loading) callbacks.onSelect(String(id));
  });
  draw.on('deselect', () => {
    if (!loading) callbacks.onSelect(undefined);
  });
  draw.on('change', (ids, type) => {
    if (type === 'delete' && !loading && !destroyed) callbacks.onDeleted(ids.map(String));
  });
  draw.start();
  draw.setMode('select');

  return {
    setTool(tool) {
      if (!destroyed) draw.setMode(tool);
    },
    load(features, selectedId) {
      if (destroyed) return;
      loading += 1;
      try {
        draw.clear();
        draw.addFeatures(
          features.map((feature) => ({
            id: feature.id,
            type: 'Feature' as const,
            geometry: feature.geometry as never,
            properties: { mode: feature.mode },
          })),
        );
        if (selectedId !== undefined && draw.getMode() === 'select') {
          draw.selectFeature(selectedId);
        }
      } finally {
        loading -= 1;
      }
    },
    select(id) {
      if (destroyed || draw.getMode() !== 'select') return;
      loading += 1;
      try {
        for (const feature of draw.getSnapshot()) {
          if (feature.id !== undefined && String(feature.id) !== id) {
            draw.deselectFeature(feature.id);
          }
        }
        if (id !== undefined) draw.selectFeature(id);
      } catch {
        // The feature is not on the drawing layer (a read-only shape).
      } finally {
        loading -= 1;
      }
    },
    destroy() {
      if (destroyed) return;
      destroyed = true;
      try {
        draw.stop();
      } catch {
        // The map may already be gone.
      }
    },
  };
}
