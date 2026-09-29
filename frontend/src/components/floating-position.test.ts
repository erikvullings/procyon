import { describe, expect, it } from 'vitest';
import { placeFloating } from './floating-position';

const bounds = { left: 8, top: 8, right: 492, bottom: 292 };
const size = { width: 200, height: 80 };

describe('placeFloating', () => {
  it('prefers below-right of the point', () => {
    expect(placeFloating({ x: 50, y: 50 }, size, bounds)).toEqual({ left: 62, top: 62 });
  });

  it('flips to the other side when the preferred side would overflow', () => {
    expect(placeFloating({ x: 450, y: 250 }, size, bounds)).toEqual({ left: 238, top: 158 });
  });

  it('clamps inside the bounds when neither side fits', () => {
    expect(placeFloating({ x: 250, y: 150 }, { width: 400, height: 80 }, bounds)).toEqual({
      left: 92,
      top: 162,
    });
    expect(placeFloating({ x: 20, y: 20 }, { width: 600, height: 80 }, bounds).left).toBe(8);
  });
});
