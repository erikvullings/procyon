import { describe, expect, it } from 'vitest';
import { overlayCorners } from './map-overlays';

const base = { id: 'a', url: 'data:image/png;base64,AA==', north: 10, south: 0, east: 10, west: 0 };

describe('overlayCorners', () => {
  it('returns the corners clockwise from the top-left when not rotated', () => {
    expect(overlayCorners(base)).toEqual([
      [0, 10],
      [10, 10],
      [10, 0],
      [0, 0],
    ]);
  });

  it('turns a quarter around the centre, keeping the distance to it', () => {
    const [topLeft, topRight] = overlayCorners({
      ...base,
      north: 1,
      south: -1,
      east: 1,
      west: -1,
      rotation: 90,
    });
    // At the equator (cos = 1) a counter-clockwise quarter turn sends the top-left to the bottom-left.
    expect(topLeft[0]).toBeCloseTo(-1, 6);
    expect(topLeft[1]).toBeCloseTo(-1, 6);
    expect(topRight[0]).toBeCloseTo(-1, 6);
    expect(topRight[1]).toBeCloseTo(1, 6);
  });
});
