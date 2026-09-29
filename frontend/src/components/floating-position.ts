export interface FloatingBounds {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

export interface FloatingSize {
  width: number;
  height: number;
}

/**
 * Places a floating box of `size` next to `point`, preferring below-right, flipping to the other
 * side of the point on an axis where it would overflow, and finally clamping inside `bounds` so it
 * is never cut off by the window or a clipping container.
 */
export function placeFloating(
  point: { x: number; y: number },
  size: FloatingSize,
  bounds: FloatingBounds,
  offset = 12,
): { left: number; top: number } {
  return {
    left: placeAxis(point.x, size.width, bounds.left, bounds.right, offset),
    top: placeAxis(point.y, size.height, bounds.top, bounds.bottom, offset),
  };
}

function placeAxis(at: number, extent: number, min: number, max: number, offset: number): number {
  const after = at + offset;
  const before = at - offset - extent;
  const preferred = after + extent <= max || before < min ? after : before;
  return Math.max(min, Math.min(preferred, max - extent));
}

/** The part of `rect` that is visible inside the window, inset by `margin`. */
export function visibleBounds(
  rect: { left: number; top: number; right: number; bottom: number },
  margin = 8,
): FloatingBounds {
  return {
    left: Math.max(rect.left, 0) + margin,
    top: Math.max(rect.top, 0) + margin,
    right: Math.min(rect.right, window.innerWidth) - margin,
    bottom: Math.min(rect.bottom, window.innerHeight) - margin,
  };
}
