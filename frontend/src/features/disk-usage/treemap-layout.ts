import { t } from '../../i18n';
import type { DiskUsageNode } from '../../models';

export interface TreemapBounds {
  readonly x: number;
  readonly y: number;
  readonly width: number;
  readonly height: number;
}

export interface TreemapRectangle {
  readonly node: DiskUsageNode;
  readonly bounds: TreemapBounds;
}

const MICRO_FILE_RATIO = 0.005;

/** Replaces individually invisible nodes with one bounded rendering item. */
export function visibleTreemapChildren(
  children: readonly DiskUsageNode[],
  parentPhysicalBytes: number,
): readonly DiskUsageNode[] {
  const threshold = parentPhysicalBytes * MICRO_FILE_RATIO;
  const visible: DiskUsageNode[] = [];
  const tiny: DiskUsageNode[] = [];
  for (const child of children) {
    if (child.physicalBytes > 0 && child.physicalBytes < threshold) tiny.push(child);
    else if (child.physicalBytes > 0) visible.push(child);
  }
  if (tiny.length > 0) {
    const first = tiny[0];
    if (first !== undefined) {
      visible.push({
        name: t('diskUsage', 'smallFiles', { count: tiny.length }),
        kind: 'file',
        location: first.location,
        logicalBytes: tiny.reduce((sum, item) => sum + item.logicalBytes, 0),
        physicalBytes: tiny.reduce((sum, item) => sum + item.physicalBytes, 0),
        children: [],
      });
    }
  }
  return visible.sort((left, right) => right.physicalBytes - left.physicalBytes);
}

function worstAspectRatio(row: readonly number[], shortSide: number): number {
  if (row.length === 0 || shortSide <= 0) return Number.POSITIVE_INFINITY;
  const sum = row.reduce((total, area) => total + area, 0);
  const largest = row[0] ?? 0;
  const smallest = row.at(-1) ?? 0;
  if (sum <= 0 || smallest <= 0) return Number.POSITIVE_INFINITY;
  const sideSquared = shortSide * shortSide;
  return Math.max((sideSquared * largest) / (sum * sum), (sum * sum) / (sideSquared * smallest));
}

function layoutRow(
  row: readonly { readonly node: DiskUsageNode; readonly area: number }[],
  available: TreemapBounds,
): { readonly rectangles: TreemapRectangle[]; readonly remaining: TreemapBounds } {
  const rowArea = row.reduce((sum, item) => sum + item.area, 0);
  const rectangles: TreemapRectangle[] = [];
  if (available.width >= available.height) {
    const rowWidth = available.height <= 0 ? 0 : rowArea / available.height;
    let y = available.y;
    for (const item of row) {
      const height = rowWidth <= 0 ? 0 : item.area / rowWidth;
      rectangles.push({
        node: item.node,
        bounds: { x: available.x, y, width: rowWidth, height },
      });
      y += height;
    }
    return {
      rectangles,
      remaining: {
        x: available.x + rowWidth,
        y: available.y,
        width: Math.max(0, available.width - rowWidth),
        height: available.height,
      },
    };
  }

  const rowHeight = available.width <= 0 ? 0 : rowArea / available.width;
  let x = available.x;
  for (const item of row) {
    const width = rowHeight <= 0 ? 0 : item.area / rowHeight;
    rectangles.push({
      node: item.node,
      bounds: { x, y: available.y, width, height: rowHeight },
    });
    x += width;
  }
  return {
    rectangles,
    remaining: {
      x: available.x,
      y: available.y + rowHeight,
      width: available.width,
      height: Math.max(0, available.height - rowHeight),
    },
  };
}

/** Squarifies nodes by physical size into non-overlapping rectangles that fill `bounds`. */
export function squarify(
  nodes: readonly DiskUsageNode[],
  bounds: TreemapBounds,
): readonly TreemapRectangle[] {
  const sorted = [...nodes]
    .filter((node) => node.physicalBytes > 0)
    .sort((left, right) => right.physicalBytes - left.physicalBytes);
  const total = sorted.reduce((sum, node) => sum + node.physicalBytes, 0);
  if (total <= 0 || bounds.width <= 0 || bounds.height <= 0) return [];
  const scale = (bounds.width * bounds.height) / total;
  const remainingItems = sorted.map((node) => ({ node, area: node.physicalBytes * scale }));
  const rectangles: TreemapRectangle[] = [];
  let available = bounds;
  let row: typeof remainingItems = [];

  while (remainingItems.length > 0) {
    const next = remainingItems[0];
    if (next === undefined) break;
    const shortSide = Math.min(available.width, available.height);
    const currentWorst = worstAspectRatio(
      row.map((item) => item.area),
      shortSide,
    );
    const nextWorst = worstAspectRatio(
      [...row, next].map((item) => item.area),
      shortSide,
    );
    if (row.length === 0 || nextWorst <= currentWorst) {
      row = [...row, next];
      remainingItems.shift();
      continue;
    }
    const laidOut = layoutRow(row, available);
    rectangles.push(...laidOut.rectangles);
    available = laidOut.remaining;
    row = [];
  }
  if (row.length > 0) rectangles.push(...layoutRow(row, available).rectangles);
  return rectangles;
}
