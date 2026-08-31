import { describe, expect, it } from 'vitest';
import type { DiskUsageNode } from '../../models';
import { squarify, visibleTreemapChildren } from './treemap-layout';

function node(name: string, physicalBytes: number): DiskUsageNode {
  return {
    name,
    kind: 'file',
    location: { providerId: 'local', uri: `file:///tmp/${name}` },
    logicalBytes: physicalBytes,
    physicalBytes,
    children: [],
  };
}

describe('squarified disk-usage layout', () => {
  it('uses the full bounds without overlap for realistically unsorted sizes', () => {
    const rectangles = squarify(
      [node('medium-a', 30), node('tiny', 5), node('large', 50), node('medium-b', 15)],
      { x: 0, y: 0, width: 100, height: 60 },
    );

    const totalArea = rectangles.reduce(
      (sum, item) => sum + item.bounds.width * item.bounds.height,
      0,
    );
    expect(totalArea).toBeCloseTo(6000, 5);
    for (const [index, rectangle] of rectangles.entries()) {
      for (const other of rectangles.slice(index + 1)) {
        const overlapWidth = Math.max(
          0,
          Math.min(
            rectangle.bounds.x + rectangle.bounds.width,
            other.bounds.x + other.bounds.width,
          ) - Math.max(rectangle.bounds.x, other.bounds.x),
        );
        const overlapHeight = Math.max(
          0,
          Math.min(
            rectangle.bounds.y + rectangle.bounds.height,
            other.bounds.y + other.bounds.height,
          ) - Math.max(rectangle.bounds.y, other.bounds.y),
        );
        expect(overlapWidth * overlapHeight).toBeCloseTo(0, 5);
      }
    }
  });

  it('aggregates children below half a percent into one small-files bucket', () => {
    const visible = visibleTreemapChildren(
      [node('large', 998), node('tiny-a', 2), node('tiny-b', 1)],
      1001,
    );

    expect(visible.map((item) => item.name)).toEqual(['large', 'Small files (2)']);
    expect(visible[1]?.physicalBytes).toBe(3);
  });
});
