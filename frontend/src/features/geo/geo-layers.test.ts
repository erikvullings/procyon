import { describe, expect, it } from 'vitest';
import type { GeoDocument } from '../../models';
import { isHidden, layersOf, visibleFeatures } from './geo-layers';

function documentOf(folders: (string | undefined)[]): GeoDocument {
  return {
    format: 'kml',
    collection: {
      type: 'FeatureCollection',
      features: folders.map((folder, index) => ({
        type: 'Feature',
        geometry: { type: 'Point', coordinates: [index, 0] },
        properties: folder === undefined ? {} : { folder },
      })),
    },
    totalFeatures: folders.length,
    displayedFeatures: folders.length,
    truncated: false,
    warnings: [],
    skipped: [],
  };
}

const document_ = documentOf([
  'Trip',
  'Trip / Day 1',
  'Trip / Day 1 / Sights',
  'Trip / Day 1 / Sights',
  undefined,
  'Trip / Day 2',
]);

describe('layersOf', () => {
  it('lists folders in tree order with direct feature counts', () => {
    expect(layersOf(document_)).toEqual([
      { path: 'Trip', name: 'Trip', depth: 0, count: 1 },
      { path: 'Trip / Day 1', name: 'Day 1', depth: 1, count: 1 },
      { path: 'Trip / Day 1 / Sights', name: 'Sights', depth: 2, count: 2 },
      { path: 'Trip / Day 2', name: 'Day 2', depth: 1, count: 1 },
    ]);
    expect(layersOf(documentOf([undefined]))).toEqual([]);
  });
});

describe('isHidden and visibleFeatures', () => {
  it('hides a folder together with everything below it', () => {
    const hidden = new Set(['Trip / Day 1']);
    expect(isHidden('Trip / Day 1 / Sights', hidden)).toBe(true);
    expect(isHidden('Trip / Day 2', hidden)).toBe(false);
    expect(isHidden('', hidden)).toBe(false);
    const { collection, originalIndexes } = visibleFeatures(document_, hidden);
    expect(originalIndexes).toEqual([0, 4, 5]);
    expect(collection.features).toHaveLength(3);
  });

  it('returns the original collection when nothing is hidden', () => {
    expect(visibleFeatures(document_, new Set()).collection).toBe(document_.collection);
  });
});
