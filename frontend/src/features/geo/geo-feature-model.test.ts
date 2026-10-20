import { describe, expect, it } from 'vitest';
import { describeValue, featureLabel, geometryKind, propertyRows } from './geo-feature-model';

const feature = (properties: Record<string, unknown> | null, id?: string | number) => ({
  type: 'Feature' as const,
  geometry: null,
  properties,
  ...(id === undefined ? {} : { id }),
});

describe('featureLabel', () => {
  it('prefers name, label, title and id, in that order and ignoring case', () => {
    expect(featureLabel(feature({ id: 7, Title: 'T', LABEL: 'L', Name: 'N' }))).toBe('N');
    expect(featureLabel(feature({ id: 7, Title: 'T', LABEL: 'L' }))).toBe('L');
    expect(featureLabel(feature({ id: 7, Title: 'T' }))).toBe('T');
    expect(featureLabel(feature({ x: 1, ID: 7 }))).toBe('7');
  });

  it('falls back to the feature id, the first scalar property, and then nothing', () => {
    expect(featureLabel(feature({ name: ' ', a: { b: 1 } }, 'f1'))).toBe('f1');
    expect(featureLabel(feature({ a: { b: 1 }, c: 'first' }))).toBe('first');
    expect(featureLabel(feature({}))).toBe('');
    expect(featureLabel(feature(null))).toBe('');
  });
});

describe('geometryKind', () => {
  it('maps every geometry type', () => {
    expect(geometryKind({ type: 'MultiLineString', coordinates: [] })).toBe('multiline');
    expect(geometryKind({ type: 'MultiPolygon', coordinates: [] })).toBe('multipolygon');
    expect(geometryKind(null)).toBe('none');
  });
});

describe('describeValue', () => {
  it('uses a fixed representation per type', () => {
    expect(describeValue(3)).toEqual({ type: 'number', text: '3', nested: false });
    expect(describeValue(true).text).toBe('true');
    expect(describeValue(null).text).toBe('null');
    expect(describeValue(['a', 2])).toEqual({ type: 'array', text: '[a, 2]', nested: false });
    expect(describeValue([{ a: 1 }])).toEqual({ type: 'array', text: '[1]', nested: true });
    expect(describeValue({ a: 1, b: 2 })).toEqual({ type: 'object', text: '{2}', nested: true });
  });
});

describe('propertyRows', () => {
  it('unfolds only expanded paths', () => {
    const properties = { a: { b: { c: 1 } }, list: [{ x: 1 }] };
    expect(propertyRows(properties, new Set()).map((row) => row.key)).toEqual(['a', 'list']);
    const open = propertyRows(properties, new Set(['/a', '/a/b', '/list']));
    expect(open.map((row) => [row.depth, row.key])).toEqual([
      [0, 'a'],
      [1, 'b'],
      [2, 'c'],
      [0, 'list'],
      [1, '0'],
    ]);
  });
});
