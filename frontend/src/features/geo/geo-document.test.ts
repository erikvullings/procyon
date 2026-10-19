import { describe, expect, it } from 'vitest';
import type { EntrySummary } from '../../models';
import { isGeoCandidate, looksLikeGeoJson, propertyText } from './geo-document';

const entry = (name: string, kind: EntrySummary['kind'] = 'file'): EntrySummary =>
  ({ name, kind, extension: name.split('.').pop() }) as EntrySummary;

describe('geo detection', () => {
  it('treats geojson and json files as candidates', () => {
    expect(isGeoCandidate(entry('a.geojson'))).toBe(true);
    expect(isGeoCandidate(entry('a.JSON'))).toBe(true);
    expect(isGeoCandidate(entry('a.gpx'))).toBe(true);
    expect(isGeoCandidate(entry('a.KMZ'))).toBe(true);
    expect(isGeoCandidate(entry('a.kml'))).toBe(true);
    expect(isGeoCandidate(entry('a.txt'))).toBe(false);
    expect(isGeoCandidate(entry('a.geojson', 'directory'))).toBe(false);
  });

  it('sniffs GeoJSON object types at the start of a json file', () => {
    expect(looksLikeGeoJson('﻿  {"type": "FeatureCollection", "features": []}')).toBe(true);
    expect(looksLikeGeoJson('{"name":"x","type":"Point"}')).toBe(true);
    expect(looksLikeGeoJson('{"type":"book"}')).toBe(false);
    expect(looksLikeGeoJson('[{"type":"Point"}]')).toBe(false);
  });
});

describe('propertyText', () => {
  it('renders primitives plainly and structures as JSON', () => {
    expect(propertyText('a')).toBe('a');
    expect(propertyText(3)).toBe('3');
    expect(propertyText(null)).toBe('');
    expect(propertyText({ a: [1] })).toBe('{"a":[1]}');
  });
});
