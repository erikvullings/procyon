import { describe, expect, it } from 'vitest';
import { fromDrawGeometry, splitFeatures } from './draw-features';
import { parseEditDocument } from './geo-edit-model';

function documentOf(features: unknown[]) {
  const parsed = parseEditDocument(JSON.stringify({ type: 'FeatureCollection', features }));
  if (!parsed.ok) throw new Error(parsed.message);
  return parsed.document;
}

const feature = (geometry: unknown) => ({ type: 'Feature', geometry, properties: {} });

describe('splitFeatures', () => {
  it('keeps simple 2D shapes editable and everything else read-only', () => {
    const document = documentOf([
      feature({ type: 'Point', coordinates: [1, 2] }),
      feature({
        type: 'LineString',
        coordinates: [
          [0, 0],
          [1, 1],
        ],
      }),
      feature({
        type: 'Polygon',
        coordinates: [
          [
            [0, 0],
            [1, 0],
            [1, 1],
            [0, 0],
          ],
        ],
      }),
      feature({ type: 'MultiPoint', coordinates: [[1, 2]] }),
      feature({ type: 'Point', coordinates: [1, 2, 30] }),
      feature({ type: 'GeometryCollection', geometries: [] }),
      feature(null),
    ]);
    const { editable, readOnly } = splitFeatures(document);
    expect(editable.map((item) => item.mode)).toEqual(['point', 'linestring', 'polygon']);
    expect(editable.map((item) => item.id)).toEqual(document.ids.slice(0, 3));
    // Features without geometry draw nothing, so they are not listed as read-only shapes.
    expect(readOnly).toHaveLength(3);
  });

  it('rejects degenerate shapes', () => {
    const { editable, readOnly } = splitFeatures(
      documentOf([
        feature({ type: 'LineString', coordinates: [[0, 0]] }),
        feature({
          type: 'Polygon',
          coordinates: [
            [
              [0, 0],
              [1, 1],
              [0, 0],
            ],
          ],
        }),
        feature({ type: 'Point', coordinates: [] }),
      ]),
    );
    expect(editable).toHaveLength(0);
    expect(readOnly).toHaveLength(3);
  });
});

describe('fromDrawGeometry', () => {
  it('rounds coordinates to about a centimetre and supports the three simple types', () => {
    expect(
      fromDrawGeometry({ type: 'Point', coordinates: [4.123456789012, 52.0000000001] }),
    ).toEqual({
      type: 'Point',
      coordinates: [4.1234568, 52],
    });
    expect(
      fromDrawGeometry({
        type: 'LineString',
        coordinates: [
          [0, 0],
          [1.00000004, 1],
        ],
      }),
    ).toEqual({
      type: 'LineString',
      coordinates: [
        [0, 0],
        [1, 1],
      ],
    });
    expect(
      fromDrawGeometry({
        type: 'Polygon',
        coordinates: [
          [
            [0, 0],
            [1, 0],
            [1, 1],
            [0, 0],
          ],
        ],
      })?.type,
    ).toBe('Polygon');
    expect(fromDrawGeometry({ type: 'MultiPoint', coordinates: [] })).toBeUndefined();
  });
});
