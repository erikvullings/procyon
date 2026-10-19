import { describe, expect, it } from 'vitest';
import { boundsOf, type MapMarker, MarkerIndex } from './marker-index';

const marker = (id: string, latitude: number, longitude: number): MapMarker => ({
  id,
  latitude,
  longitude,
});

const WORLD = [-180, -85, 180, 85] as const;

describe('MarkerIndex', () => {
  const markers = [marker('a', 52.0, 4.0), marker('b', 52.0001, 4.0001), marker('c', -33.9, 151.2)];

  it('groups nearby markers when zoomed out and separates them when zoomed in', () => {
    const index = new MarkerIndex(markers, 18);
    const far = index.viewport(WORLD, 2);
    expect(far.filter((item) => item.kind === 'cluster')).toHaveLength(1);
    expect(far.filter((item) => item.kind === 'marker')).toHaveLength(1);
    const cluster = far.find((item) => item.kind === 'cluster');
    expect(cluster).toMatchObject({ count: 2 });
    const near = index.viewport(WORLD, 18);
    expect(near.every((item) => item.kind === 'marker')).toBe(true);
    expect(near).toHaveLength(3);
  });

  it('reports the zoom level at which a cluster breaks up', () => {
    const index = new MarkerIndex(markers, 18);
    const cluster = index.viewport(WORLD, 2).find((item) => item.kind === 'cluster');
    if (cluster?.kind !== 'cluster') throw new Error('cluster expected');
    expect(index.expansionZoom(cluster.clusterId)).toBeGreaterThan(2);
  });

  it('only returns items inside the viewport and ignores unusable coordinates', () => {
    const index = new MarkerIndex([...markers, marker('nan', Number.NaN, 0)], 18);
    expect(index.viewport([140, -40, 160, -30], 18)).toHaveLength(1);
    expect(index.viewport(WORLD, 18)).toHaveLength(3);
  });
});

describe('boundsOf', () => {
  it('encloses all markers', () => {
    expect(boundsOf([marker('a', 10, 20), marker('b', -5, 30)])).toEqual([20, -5, 30, 10]);
    expect(boundsOf([])).toBeUndefined();
  });
});
