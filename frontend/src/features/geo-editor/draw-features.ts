import type { DrawGeometry, GeoEditDocument, JsonObject } from './geo-edit-model';

/** A feature the drawing library can edit: a 2D Point, LineString or Polygon. */
export interface EditableDrawFeature {
  readonly id: string;
  readonly mode: 'point' | 'linestring' | 'polygon';
  readonly geometry: DrawGeometry;
}

function isPosition(value: unknown): value is [number, number] {
  return (
    Array.isArray(value) &&
    value.length === 2 &&
    typeof value[0] === 'number' &&
    typeof value[1] === 'number' &&
    Number.isFinite(value[0]) &&
    Number.isFinite(value[1])
  );
}

function allPositions(value: unknown): value is [number, number][] {
  return Array.isArray(value) && value.every(isPosition);
}

function editableGeometry(geometry: unknown): EditableDrawFeature['mode'] | undefined {
  if (typeof geometry !== 'object' || geometry === null) return undefined;
  const { type, coordinates } = geometry as { type?: unknown; coordinates?: unknown };
  if (type === 'Point') return isPosition(coordinates) ? 'point' : undefined;
  if (type === 'LineString') {
    return allPositions(coordinates) && coordinates.length >= 2 ? 'linestring' : undefined;
  }
  if (type === 'Polygon') {
    return Array.isArray(coordinates) &&
      coordinates.length > 0 &&
      coordinates.every((ring) => allPositions(ring) && ring.length >= 4)
      ? 'polygon'
      : undefined;
  }
  return undefined;
}

/**
 * Splits the document into features the drawing library edits and everything else (multi
 * geometries, collections, 3D positions, features without geometry), which stays read-only on the
 * map and is edited in the JSON pane.
 */
export function splitFeatures(document: GeoEditDocument): {
  readonly editable: readonly EditableDrawFeature[];
  readonly readOnly: readonly JsonObject[];
} {
  const editable: EditableDrawFeature[] = [];
  const readOnly: JsonObject[] = [];
  document.features.forEach((feature, index) => {
    const id = document.ids[index];
    const mode = editableGeometry(feature.geometry);
    if (id !== undefined && mode !== undefined) {
      editable.push({ id, mode, geometry: feature.geometry as DrawGeometry });
    } else if (feature.geometry !== null && feature.geometry !== undefined) {
      readOnly.push(feature);
    }
  });
  return { editable, readOnly };
}

const PRECISION = 1e7;

function round(value: number): number {
  return Math.round(value * PRECISION) / PRECISION;
}

/** Converts a geometry coming out of the drawing library; undefined for unsupported types. */
export function fromDrawGeometry(geometry: unknown): DrawGeometry | undefined {
  if (editableGeometry(geometry) === undefined) return undefined;
  const { type, coordinates } = geometry as { type: string; coordinates: unknown };
  const roundPosition = (position: readonly number[]): number[] =>
    position.map((value) => round(value));
  if (type === 'Point') {
    return { type: 'Point', coordinates: roundPosition(coordinates as number[]) };
  }
  if (type === 'LineString') {
    return {
      type: 'LineString',
      coordinates: (coordinates as number[][]).map(roundPosition),
    };
  }
  return {
    type: 'Polygon',
    coordinates: (coordinates as number[][][]).map((ring) => ring.map(roundPosition)),
  };
}
