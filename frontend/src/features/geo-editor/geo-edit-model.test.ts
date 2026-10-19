import { describe, expect, it } from 'vitest';
import { GeoEditHistory } from './geo-edit-history';
import {
  addFeature,
  circlePolygon,
  deleteProperty,
  duplicateFeature,
  type GeoEditDocument,
  importFeatures,
  parseEditDocument,
  parseTypedValue,
  propertiesOf,
  removeFeature,
  renameProperty,
  replaceGeometry,
  serializeEditDocument,
  setProperty,
  typeOfValue,
} from './geo-edit-model';

function documentOf(text: string): GeoEditDocument {
  const parsed = parseEditDocument(text);
  if (!parsed.ok) throw new Error(parsed.message);
  return parsed.document;
}

const SAMPLE = `{"type":"FeatureCollection","name":"keep me","features":[
  {"type":"Feature","id":7,"geometry":{"type":"Point","coordinates":[4,52]},"properties":{"a":1,"b":"x"}},
  {"type":"Feature","geometry":{"type":"LineString","coordinates":[[0,0],[1,1]]},"properties":null}
]}`;

describe('parseEditDocument', () => {
  it('keeps unknown members and gives every feature an editor id', () => {
    const document = documentOf(SAMPLE);
    expect(document.features).toHaveLength(2);
    expect(new Set(document.ids).size).toBe(2);
    expect(JSON.parse(serializeEditDocument(document))).toMatchObject({
      name: 'keep me',
      features: [{ id: 7 }, {}],
    });
    expect(serializeEditDocument(document).endsWith('\n')).toBe(true);
  });

  it('wraps a bare feature or geometry in a collection', () => {
    const feature = parseEditDocument('{"type":"Feature","geometry":null,"properties":{}}');
    expect(feature.ok && feature.wrapped).toBe(true);
    const geometry = parseEditDocument('{"type":"Point","coordinates":[1,2]}');
    expect(geometry.ok && geometry.document.features[0]?.geometry).toEqual({
      type: 'Point',
      coordinates: [1, 2],
    });
  });

  it('rejects text that is not a GeoJSON object', () => {
    expect(parseEditDocument('{').ok).toBe(false);
    expect(parseEditDocument('[1]').ok).toBe(false);
    expect(parseEditDocument('{"type":"FeatureCollection"}').ok).toBe(false);
    expect(parseEditDocument('{"type":"FeatureCollection","features":[1]}').ok).toBe(false);
  });
});

describe('feature operations', () => {
  it('adds, replaces the geometry of, and removes features without touching the original', () => {
    const original = documentOf(SAMPLE);
    const added = addFeature(original, { type: 'Point', coordinates: [9, 9] }, { n: 1 }, 'new');
    expect(original.features).toHaveLength(2);
    expect(added.ids.at(-1)).toBe('new');
    const moved = replaceGeometry(added, 'new', { type: 'Point', coordinates: [1, 1] });
    expect(moved.features.at(-1)?.geometry).toEqual({ type: 'Point', coordinates: [1, 1] });
    expect(removeFeature(moved, 'new').features).toHaveLength(2);
    expect(removeFeature(moved, 'missing')).toBe(moved);
  });

  it('duplicates next to the source, shifted, without the id member', () => {
    const original = documentOf(SAMPLE);
    const firstId = original.ids[0] as string;
    const copy = duplicateFeature(original, firstId, 'copy');
    expect(copy.ids).toEqual([firstId, 'copy', original.ids[1]]);
    const duplicate = copy.features[1] as Record<string, unknown>;
    expect(duplicate.id).toBeUndefined();
    expect((duplicate.geometry as { coordinates: number[] }).coordinates[0]).toBeCloseTo(4.0005);
    expect(duplicate.properties).toEqual({ a: 1, b: 'x' });
    expect(duplicate.properties).not.toBe(propertiesOf(original.features[0]));
  });

  it('imports by appending, or replacing an empty document', () => {
    const base = documentOf(SAMPLE);
    const other = documentOf('{"type":"Point","coordinates":[5,5]}');
    expect(importFeatures(base, other).features).toHaveLength(3);
    const empty = documentOf('{"type":"FeatureCollection","features":[]}');
    expect(importFeatures(empty, other).features).toHaveLength(1);
  });
});

describe('properties', () => {
  it('sets, renames (keeping order) and deletes keys', () => {
    const document = documentOf(SAMPLE);
    const id = document.ids[0] as string;
    const added = setProperty(document, id, 'c', true);
    expect(Object.keys(propertiesOf(added.features[0]))).toEqual(['a', 'b', 'c']);
    const renamed = renameProperty(added, id, 'b', 'bee');
    expect(Object.keys(propertiesOf(renamed.features[0]))).toEqual(['a', 'bee', 'c']);
    expect(renameProperty(renamed, id, 'a', 'c')).toBe(renamed);
    expect(Object.keys(propertiesOf(deleteProperty(renamed, id, 'a').features[0]))).toEqual([
      'bee',
      'c',
    ]);
  });

  it('creates properties on a feature that had null properties', () => {
    const document = documentOf(SAMPLE);
    const id = document.ids[1] as string;
    expect(propertiesOf(setProperty(document, id, 'k', 1).features[1])).toEqual({ k: 1 });
  });

  it('parses typed values and reports bad input', () => {
    expect(parseTypedValue('3.5', 'number')).toEqual({ ok: true, value: 3.5 });
    expect(parseTypedValue('', 'number').ok).toBe(false);
    expect(parseTypedValue('TRUE', 'boolean')).toEqual({ ok: true, value: true });
    expect(parseTypedValue('yes', 'boolean').ok).toBe(false);
    expect(parseTypedValue('ignored', 'null')).toEqual({ ok: true, value: null });
    expect(parseTypedValue('{"a":[1]}', 'json')).toEqual({ ok: true, value: { a: [1] } });
    expect(parseTypedValue('{', 'json').ok).toBe(false);
    expect(parseTypedValue(' 12 ', 'string')).toEqual({ ok: true, value: ' 12 ' });
    expect([1, 'a', true, null, [1]].map(typeOfValue)).toEqual([
      'number',
      'string',
      'boolean',
      'null',
      'json',
    ]);
  });
});

describe('circlePolygon', () => {
  it('builds a closed ring at the requested radius', () => {
    const circle = circlePolygon(4.9, 52.37, 1000, 32);
    expect(circle.type).toBe('Polygon');
    const ring = circle.coordinates[0] as number[][];
    expect(ring).toHaveLength(33);
    expect(ring[0]).toEqual(ring[32]);
    // The northern-most point is about 1 km (0.009 degrees of latitude) from the centre.
    expect(Math.max(...ring.map((point) => point[1] as number)) - 52.37).toBeCloseTo(0.009, 3);
  });
});

describe('GeoEditHistory', () => {
  it('undoes and redoes, and drops redo steps after a new change', () => {
    const start = documentOf(SAMPLE);
    const history = new GeoEditHistory(start);
    const one = history.push(addFeature(start, { type: 'Point', coordinates: [1, 1] }));
    const two = history.push(addFeature(one, { type: 'Point', coordinates: [2, 2] }));
    expect(history.canUndo).toBe(true);
    expect(history.undo()).toBe(one);
    expect(history.undo()).toBe(start);
    expect(history.canUndo).toBe(false);
    expect(history.redo()).toBe(one);
    expect(history.canRedo).toBe(true);
    const three = history.push(removeFeature(one, one.ids[0] as string));
    expect(history.canRedo).toBe(false);
    expect(history.document).toBe(three);
    expect(two).not.toBe(three);
  });

  it('ignores no-op pushes and bounds its size', () => {
    const start = documentOf(SAMPLE);
    const history = new GeoEditHistory(start, 3);
    history.push(start);
    expect(history.canUndo).toBe(false);
    let current = start;
    for (let step = 0; step < 6; step += 1) {
      current = history.push(addFeature(current, { type: 'Point', coordinates: [step, 0] }));
    }
    let undone = 0;
    while (history.canUndo) {
      history.undo();
      undone += 1;
    }
    expect(undone).toBe(3);
  });
});

describe('replaceGeometry', () => {
  it('leaves other members of the feature alone', () => {
    const document = documentOf(SAMPLE);
    const id = document.ids[0] as string;
    const changed = replaceGeometry(document, id, { type: 'Point', coordinates: [0, 0] });
    expect((changed.features[0] as Record<string, unknown>).id).toBe(7);
  });
});
