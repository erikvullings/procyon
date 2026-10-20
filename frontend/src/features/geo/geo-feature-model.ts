import type { GeoDocument } from '../../models';

type Feature = GeoDocument['collection']['features'][number];

/** The shape a list row is drawn with: the geometry type, or `none` when a feature has no geometry. */
export type GeometryKind =
  | 'point'
  | 'multipoint'
  | 'line'
  | 'multiline'
  | 'polygon'
  | 'multipolygon'
  | 'collection'
  | 'none';

export function geometryKind(geometry: Feature['geometry']): GeometryKind {
  switch (geometry?.type) {
    case 'Point':
      return 'point';
    case 'MultiPoint':
      return 'multipoint';
    case 'LineString':
      return 'line';
    case 'MultiLineString':
      return 'multiline';
    case 'Polygon':
      return 'polygon';
    case 'MultiPolygon':
      return 'multipolygon';
    case 'GeometryCollection':
      return 'collection';
    default:
      return 'none';
  }
}

const LABEL_KEYS = ['name', 'label', 'title', 'id'] as const;

function scalarText(value: unknown): string | undefined {
  if (typeof value === 'string') return value.trim() === '' ? undefined : value;
  if (typeof value === 'number' || typeof value === 'boolean') return String(value);
  return undefined;
}

/** A name property, label, title, id or the first scalar property; empty when none exists. */
export function featureLabel(feature: Feature): string {
  const properties = feature.properties ?? {};
  const byLowerKey = new Map(Object.keys(properties).map((key) => [key.toLowerCase(), key]));
  for (const wanted of LABEL_KEYS) {
    const key = byLowerKey.get(wanted);
    const text = key === undefined ? undefined : scalarText(properties[key]);
    if (text !== undefined) return text;
  }
  const own = scalarText(feature.id);
  if (own !== undefined) return own;
  for (const value of Object.values(properties)) {
    const text = scalarText(value);
    if (text !== undefined) return text;
  }
  return '';
}

export type ValueType = 'string' | 'number' | 'boolean' | 'null' | 'array' | 'object';

export interface ValueView {
  readonly type: ValueType;
  /** Fixed one-line representation. */
  readonly text: string;
  /** Whether the value is shown as an expandable tree (objects, and arrays holding objects). */
  readonly nested: boolean;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function inline(value: unknown): string {
  if (typeof value === 'string') return value;
  if (value === null || value === undefined) return 'null';
  if (typeof value === 'object') return JSON.stringify(value);
  return String(value);
}

export function describeValue(value: unknown): ValueView {
  if (value === null || value === undefined) return { type: 'null', text: 'null', nested: false };
  if (typeof value === 'string') return { type: 'string', text: value, nested: false };
  if (typeof value === 'number') return { type: 'number', text: String(value), nested: false };
  if (typeof value === 'boolean') return { type: 'boolean', text: String(value), nested: false };
  if (Array.isArray(value)) {
    const nested = value.some((item) => typeof item === 'object' && item !== null);
    return {
      type: 'array',
      text: nested ? `[${value.length}]` : `[${value.map(inline).join(', ')}]`,
      nested,
    };
  }
  if (isRecord(value)) {
    return { type: 'object', text: `{${Object.keys(value).length}}`, nested: true };
  }
  return { type: 'string', text: String(value), nested: false };
}

/** One row of the property tree, flattened so that rows keep a fixed height. */
export interface PropertyRow {
  readonly path: string;
  readonly depth: number;
  readonly key: string;
  readonly view: ValueView;
  readonly expanded: boolean;
}

/** Flattens properties into rows; children of a nested value follow it only when its path is expanded. */
export function propertyRows(
  properties: Readonly<Record<string, unknown>> | null | undefined,
  expanded: ReadonlySet<string>,
): PropertyRow[] {
  const rows: PropertyRow[] = [];
  const visit = (
    entries: readonly (readonly [string, unknown])[],
    depth: number,
    parent: string,
  ) => {
    for (const [key, value] of entries) {
      const path = `${parent}/${key}`;
      const view = describeValue(value);
      const open = view.nested && expanded.has(path);
      rows.push({ path, depth, key, view, expanded: open });
      if (open) {
        const children = Array.isArray(value)
          ? value.map((item, index) => [String(index), item] as const)
          : Object.entries(value as Record<string, unknown>);
        visit(children, depth + 1, path);
      }
    }
  };
  visit(Object.entries(properties ?? {}), 0, '');
  return rows;
}
