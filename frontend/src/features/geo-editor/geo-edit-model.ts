/**
 * The GeoJSON document edited on the map (task 0278), kept as plain parsed JSON so members the
 * editor does not understand (`bbox`, `name`, foreign members) survive a save. Operations return
 * new documents; nothing mutates in place, which makes undo a stack of references.
 */

import { t } from '../../i18n';

export type JsonObject = Record<string, unknown>;

/** A feature collection plus a stable editor id per feature (ids are never written to the file). */
export interface GeoEditDocument {
  readonly root: JsonObject;
  readonly features: readonly JsonObject[];
  readonly ids: readonly string[];
}

export type GeoEditParse =
  | { readonly ok: true; readonly document: GeoEditDocument; readonly wrapped: boolean }
  | {
      readonly ok: false;
      readonly message: string;
      readonly line?: number;
      readonly column?: number;
    };

export type DrawGeometry =
  | { readonly type: 'Point'; readonly coordinates: readonly number[] }
  | { readonly type: 'LineString'; readonly coordinates: readonly (readonly number[])[] }
  | { readonly type: 'Polygon'; readonly coordinates: readonly (readonly (readonly number[])[])[] };

let idCounter = 0;
export function newEditorId(): string {
  idCounter += 1;
  return `geo-${Date.now().toString(36)}-${idCounter}`;
}

function isObject(value: unknown): value is JsonObject {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function positionOf(message: string): { line?: number; column?: number } {
  const lineMatch = /line (\d+) column (\d+)/.exec(message);
  return lineMatch === null ? {} : { line: Number(lineMatch[1]), column: Number(lineMatch[2]) };
}

/** Parses editor text into a document; a bare Feature or geometry is wrapped in a collection. */
export function parseEditDocument(text: string): GeoEditParse {
  let value: unknown;
  try {
    value = JSON.parse(text);
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    return { ok: false, message, ...positionOf(message) };
  }
  if (!isObject(value) || typeof value.type !== 'string') {
    return { ok: false, message: t('geoEdit', 'parseNotObject') };
  }
  let root: JsonObject;
  let wrapped = false;
  if (value.type === 'FeatureCollection') {
    if (!Array.isArray(value.features)) {
      return { ok: false, message: t('geoEdit', 'parseNeedsFeatures') };
    }
    root = value;
  } else if (value.type === 'Feature') {
    root = { type: 'FeatureCollection', features: [value] };
    wrapped = true;
  } else {
    root = {
      type: 'FeatureCollection',
      features: [{ type: 'Feature', geometry: value, properties: {} }],
    };
    wrapped = true;
  }
  const features: JsonObject[] = [];
  for (const [index, item] of (root.features as unknown[]).entries()) {
    if (!isObject(item) || item.type !== 'Feature') {
      return { ok: false, message: t('geoEdit', 'parseNotFeature', { index }) };
    }
    features.push(item);
  }
  return {
    ok: true,
    wrapped,
    document: { root, features, ids: features.map(() => newEditorId()) },
  };
}

/** The JSON text written back to the file. */
export function serializeEditDocument(document: GeoEditDocument, pretty = true): string {
  const value = { ...document.root, features: document.features };
  return pretty ? `${JSON.stringify(value, null, 2)}\n` : `${JSON.stringify(value)}\n`;
}

function withFeatures(
  document: GeoEditDocument,
  features: readonly JsonObject[],
  ids: readonly string[],
): GeoEditDocument {
  return { root: document.root, features, ids };
}

export function indexOfId(document: GeoEditDocument, id: string | undefined): number {
  return id === undefined ? -1 : document.ids.indexOf(id);
}

/** Adds a drawn feature at the end. */
export function addFeature(
  document: GeoEditDocument,
  geometry: DrawGeometry,
  properties: JsonObject = {},
  id: string = newEditorId(),
): GeoEditDocument {
  return withFeatures(
    document,
    [...document.features, { type: 'Feature', geometry, properties }],
    [...document.ids, id],
  );
}

export function removeFeature(document: GeoEditDocument, id: string): GeoEditDocument {
  const index = indexOfId(document, id);
  if (index < 0) return document;
  return withFeatures(
    document,
    document.features.filter((_, position) => position !== index),
    document.ids.filter((_, position) => position !== index),
  );
}

/** Moves a whole feature by the given degrees (keyboard nudging). */
export function nudgeFeature(
  document: GeoEditDocument,
  id: string,
  dLongitude: number,
  dLatitude: number,
): GeoEditDocument {
  const index = indexOfId(document, id);
  const current = document.features[index];
  if (current === undefined || current.geometry === null || current.geometry === undefined) {
    return document;
  }
  return withFeatures(
    document,
    document.features.map((feature, position) =>
      position === index
        ? { ...current, geometry: translateCoordinates(current.geometry, dLongitude, dLatitude) }
        : feature,
    ),
    document.ids,
  );
}

export function replaceGeometry(
  document: GeoEditDocument,
  id: string,
  geometry: DrawGeometry,
): GeoEditDocument {
  const index = indexOfId(document, id);
  const current = document.features[index];
  if (current === undefined) return document;
  return withFeatures(
    document,
    document.features.map((feature, position) =>
      position === index ? { ...current, geometry } : feature,
    ),
    document.ids,
  );
}

/** Moves every position of a geometry (or of nested coordinates) by the given degrees. */
export function translateCoordinates(
  value: unknown,
  dLongitude: number,
  dLatitude: number,
): unknown {
  if (Array.isArray(value)) {
    if (value.length >= 2 && typeof value[0] === 'number' && typeof value[1] === 'number') {
      return [value[0] + dLongitude, value[1] + dLatitude, ...value.slice(2)];
    }
    return value.map((item) => translateCoordinates(item, dLongitude, dLatitude));
  }
  if (isObject(value)) {
    return Object.fromEntries(
      Object.entries(value).map(([key, item]) => [
        key,
        key === 'coordinates' || key === 'geometries'
          ? translateCoordinates(item, dLongitude, dLatitude)
          : item,
      ]),
    );
  }
  return value;
}

/** Distance (degrees) a duplicate is shifted so that it is visible next to its original. */
export const DUPLICATE_OFFSET_DEGREES = 0.0005;

/** Copies a feature, shifted a little so that it does not hide the original. */
export function duplicateFeature(
  document: GeoEditDocument,
  id: string,
  newId: string = newEditorId(),
): GeoEditDocument {
  const index = indexOfId(document, id);
  const source = document.features[index];
  if (source === undefined) return document;
  const copy: JsonObject = {
    ...source,
    geometry: translateCoordinates(
      source.geometry,
      DUPLICATE_OFFSET_DEGREES,
      -DUPLICATE_OFFSET_DEGREES,
    ),
    properties: isObject(source.properties) ? { ...source.properties } : {},
  };
  delete copy.id;
  return withFeatures(
    document,
    [...document.features.slice(0, index + 1), copy, ...document.features.slice(index + 1)],
    [...document.ids.slice(0, index + 1), newId, ...document.ids.slice(index + 1)],
  );
}

/** Appends the features of `other` (an import); an empty document is replaced instead. */
export function importFeatures(document: GeoEditDocument, other: GeoEditDocument): GeoEditDocument {
  if (document.features.length === 0)
    return { ...other, root: { ...document.root, ...other.root } };
  return withFeatures(
    document,
    [...document.features, ...other.features],
    [...document.ids, ...other.ids],
  );
}

/* ------------------------------- properties ------------------------------- */

export type PropertyType = 'string' | 'number' | 'boolean' | 'null' | 'json';

export function propertiesOf(feature: JsonObject | undefined): JsonObject {
  return feature !== undefined && isObject(feature.properties) ? feature.properties : {};
}

export function typeOfValue(value: unknown): PropertyType {
  if (value === null) return 'null';
  if (typeof value === 'string') return 'string';
  if (typeof value === 'number') return 'number';
  if (typeof value === 'boolean') return 'boolean';
  return 'json';
}

/** Text shown for a value in the table. */
export function valueText(value: unknown): string {
  if (value === null) return 'null';
  if (typeof value === 'string') return value;
  if (typeof value === 'number' || typeof value === 'boolean') return String(value);
  return JSON.stringify(value);
}

export type ParsedValue =
  | { readonly ok: true; readonly value: unknown }
  | { readonly ok: false; readonly message: string };

/** Converts typed text from the property table into a JSON value. */
export function parseTypedValue(text: string, type: PropertyType): ParsedValue {
  switch (type) {
    case 'string':
      return { ok: true, value: text };
    case 'number': {
      const value = Number(text.trim());
      return text.trim() !== '' && Number.isFinite(value)
        ? { ok: true, value }
        : { ok: false, message: t('geoEdit', 'enterNumber') };
    }
    case 'boolean': {
      const lowered = text.trim().toLowerCase();
      return lowered === 'true' || lowered === 'false'
        ? { ok: true, value: lowered === 'true' }
        : { ok: false, message: t('geoEdit', 'enterBoolean') };
    }
    case 'null':
      return { ok: true, value: null };
    case 'json':
      try {
        return { ok: true, value: JSON.parse(text) };
      } catch {
        return { ok: false, message: t('geoEdit', 'enterJson') };
      }
  }
}

function withProperties(
  document: GeoEditDocument,
  id: string,
  update: (properties: JsonObject) => JsonObject,
): GeoEditDocument {
  const index = indexOfId(document, id);
  const current = document.features[index];
  if (current === undefined) return document;
  return withFeatures(
    document,
    document.features.map((feature, position) =>
      position === index ? { ...current, properties: update(propertiesOf(current)) } : feature,
    ),
    document.ids,
  );
}

/** Sets a property, keeping its position among the keys when it already exists. */
export function setProperty(
  document: GeoEditDocument,
  id: string,
  key: string,
  value: unknown,
): GeoEditDocument {
  return withProperties(document, id, (properties) => ({ ...properties, [key]: value }));
}

export function deleteProperty(
  document: GeoEditDocument,
  id: string,
  key: string,
): GeoEditDocument {
  return withProperties(document, id, (properties) => {
    const { [key]: _removed, ...rest } = properties;
    return rest;
  });
}

/** Renames a key in place (order preserved); refuses when the new key already exists. */
export function renameProperty(
  document: GeoEditDocument,
  id: string,
  from: string,
  to: string,
): GeoEditDocument {
  if (from === to || to.trim() === '') return document;
  const existing = propertiesOf(document.features[indexOfId(document, id)]);
  if (!(from in existing) || to in existing) return document;
  return withProperties(document, id, (properties) =>
    Object.fromEntries(
      Object.entries(properties).map(([key, value]) => [key === from ? to : key, value]),
    ),
  );
}

/* ---------------------------------- shapes --------------------------------- */

const EARTH_RADIUS_METERS = 6_371_008.8;

/** A closed polygon approximating a circle (GeoJSON has no circle), as a ring of `steps` points. */
export function circlePolygon(
  longitude: number,
  latitude: number,
  radiusMeters: number,
  steps = 64,
): DrawGeometry {
  const angular = radiusMeters / EARTH_RADIUS_METERS;
  const lat = (latitude * Math.PI) / 180;
  const lon = (longitude * Math.PI) / 180;
  const ring: number[][] = [];
  for (let step = 0; step < steps; step += 1) {
    const bearing = (2 * Math.PI * step) / steps;
    const pointLat = Math.asin(
      Math.sin(lat) * Math.cos(angular) + Math.cos(lat) * Math.sin(angular) * Math.cos(bearing),
    );
    const pointLon =
      lon +
      Math.atan2(
        Math.sin(bearing) * Math.sin(angular) * Math.cos(lat),
        Math.cos(angular) - Math.sin(lat) * Math.sin(pointLat),
      );
    ring.push([(pointLon * 180) / Math.PI, (pointLat * 180) / Math.PI]);
  }
  const [first] = ring;
  if (first !== undefined) ring.push([...first]);
  return { type: 'Polygon', coordinates: [ring] };
}
