/**
 * WinDirStat-style cushion treemap: layout into paint operations plus a pure pixel painter.
 *
 * Approach adapted from BlitzTree's renderer (https://github.com/ahmedkhaleel2004/blitztree,
 * MIT, © 2026 Ahmed Khaleel): every tile adds a parabolic ridge to an accumulated quadratic
 * surface `z = ax2·x² + ax1·x + ay2·y² + ay1·y`, and each pixel is shaded once from the surface
 * normal of the last tile painted over it. Parents paint before children, so every pixel is
 * covered and nesting reads through the compounded cushions.
 */
import type { DiskUsageNode } from '../../models';
import { nodeColour, type Rgb, type TreemapPalette } from './file-type-colours';
import { squarify, type TreemapBounds, visibleTreemapChildren } from './treemap-layout';

export interface CushionSurface {
  readonly ax2: number;
  readonly ax1: number;
  readonly ay2: number;
  readonly ay1: number;
}

/** Paints `rgb` over the pixel box; `surface` undefined means flat (unshaded). */
export interface ShadeOp {
  readonly type: 'shade';
  readonly x0: number;
  readonly y0: number;
  readonly x1: number;
  readonly y1: number;
  readonly rgb: Rgb;
  readonly surface: CushionSurface | undefined;
}

/** Darkens a band `thickness` pixels wide just inside the box, unless a later shade covers it. */
export interface FrameOp {
  readonly type: 'frame';
  readonly x0: number;
  readonly y0: number;
  readonly x1: number;
  readonly y1: number;
  readonly thickness: number;
  readonly factor: number;
}

export type PaintOp = ShadeOp | FrameOp;

export interface TreemapTile {
  readonly node: DiskUsageNode;
  /** CSS pixels. */
  readonly bounds: TreemapBounds;
  readonly depth: number;
  /** Nodes from the scene root's child down to `node`, inclusive. */
  readonly trail: readonly DiskUsageNode[];
}

export interface TreemapLabel {
  readonly text: string;
  /** Title strip in CSS pixels. */
  readonly bounds: TreemapBounds;
  readonly node: DiskUsageNode;
}

export interface TreemapScene {
  /** Device pixels. */
  readonly width: number;
  readonly height: number;
  readonly scale: number;
  readonly ops: readonly PaintOp[];
  /** Paint order: parents before their children, so the last containing tile is the deepest. */
  readonly tiles: readonly TreemapTile[];
  readonly labels: readonly TreemapLabel[];
}

const BASE_HEIGHT = 0.55;
const FALLOFF = 0.72;
const AMBIENT = 0.38;
const LIGHT_X = -0.408;
const LIGHT_Y = -0.408;
const LIGHT_Z = 0.816;
const MAX_BRIGHTNESS = 1.22;
/** Canvas can afford far finer tiles than SVG; sub-pixel tiles are still dropped. */
const CANVAS_MICRO_RATIO = 0.0005;
const HEADER_CSS_HEIGHT = 16;
const FRAME_FACTORS = [0.5, 0.62, 0.72, 0.8] as const;
/** A chain of directories each holding at least this share of their parent renders as one strip. */
const CHAIN_SHARE = 0.99;

const FLAT: CushionSurface = { ax2: 0, ax1: 0, ay2: 0, ay1: 0 };

export function addRidge(surface: CushionSurface, rect: TreemapBounds, height: number) {
  let { ax2, ax1, ay2, ay1 } = surface;
  if (rect.width > 0) {
    const h4 = (4 * height) / (rect.width * rect.width);
    ax2 -= h4;
    ax1 += h4 * (2 * rect.x + rect.width);
  }
  if (rect.height > 0) {
    const h4 = (4 * height) / (rect.height * rect.height);
    ay2 -= h4;
    ay1 += h4 * (2 * rect.y + rect.height);
  }
  return { ax2, ax1, ay2, ay1 };
}

/** Brightness multiplier at a pixel centre; 1 on a flat surface. */
export function cushionBrightness(surface: CushionSurface, x: number, y: number): number {
  const nx = -(2 * surface.ax2 * x + surface.ax1);
  const ny = -(2 * surface.ay2 * y + surface.ay1);
  const cosine = (nx * LIGHT_X + ny * LIGHT_Y + LIGHT_Z) / Math.sqrt(nx * nx + ny * ny + 1);
  return Math.min(MAX_BRIGHTNESS, AMBIENT + ((1 - AMBIENT) * Math.max(0, cosine)) / LIGHT_Z);
}

function pixelBox(rect: TreemapBounds) {
  return {
    x0: Math.round(rect.x),
    y0: Math.round(rect.y),
    x1: Math.round(rect.x + rect.width),
    y1: Math.round(rect.y + rect.height),
  };
}

function largestChild(node: DiskUsageNode): DiskUsageNode | undefined {
  let largest: DiskUsageNode | undefined;
  for (const child of node.children) {
    if (largest === undefined || child.physicalBytes > largest.physicalBytes) largest = child;
  }
  return largest;
}

/** Lays `root` out over a `cssWidth`×`cssHeight` canvas rendered at `scale` device pixels. */
export function buildTreemapScene(
  root: DiskUsageNode,
  cssWidth: number,
  cssHeight: number,
  scale: number,
  palette: TreemapPalette,
): TreemapScene {
  const width = Math.max(0, Math.round(cssWidth * scale));
  const height = Math.max(0, Math.round(cssHeight * scale));
  const ops: PaintOp[] = [];
  const tiles: TreemapTile[] = [];
  const labels: TreemapLabel[] = [];
  const headerHeight = Math.round(HEADER_CSS_HEIGHT * scale);
  const toCss = (rect: TreemapBounds): TreemapBounds => ({
    x: rect.x / scale,
    y: rect.y / scale,
    width: rect.width / scale,
    height: rect.height / scale,
  });
  const shade = (rect: TreemapBounds, rgb: Rgb, surface: CushionSurface | undefined) => {
    const box = pixelBox(rect);
    if (box.x1 > box.x0 && box.y1 > box.y0) ops.push({ type: 'shade', ...box, rgb, surface });
  };
  const frame = (rect: TreemapBounds, thickness: number, factor: number) => {
    const box = pixelBox(rect);
    if (box.x1 > box.x0 && box.y1 > box.y0 && thickness > 0) {
      ops.push({ type: 'frame', ...box, thickness, factor });
    }
  };

  const draw = (
    node: DiskUsageNode,
    rect: TreemapBounds,
    ridge: number,
    surface: CushionSurface,
    depth: number,
    trail: readonly DiskUsageNode[],
  ): void => {
    if (rect.width < 0.5 || rect.height < 0.5) return;
    // The view root adds no ridge: a canvas-wide parabola would only vignette the map.
    const own =
      depth > 0 ? addRidge(surface, rect, ridge * Math.min(rect.width, rect.height)) : surface;
    if (depth > 0) tiles.push({ node, bounds: toCss(rect), depth, trail });
    const expandable = node.kind === 'directory' && !node.collapsed && node.children.length > 0;
    if (!expandable) {
      shade(rect, nodeColour(node, palette), own);
      return;
    }

    const headed =
      depth >= 1 &&
      rect.width >= 88 * scale &&
      rect.height >= Math.max(58 * scale, headerHeight * 2.8);
    let content = rect;
    let layoutNode = node;
    let layoutTrail = trail;
    if (headed) {
      let text = node.name;
      for (;;) {
        const next = largestChild(layoutNode);
        if (
          next === undefined ||
          next.kind !== 'directory' ||
          next.collapsed ||
          next.children.length === 0 ||
          next.physicalBytes < CHAIN_SHARE * Math.max(layoutNode.physicalBytes, 1)
        ) {
          break;
        }
        text += `  ▸  ${next.name}`;
        layoutNode = next;
        layoutTrail = [...layoutTrail, next];
      }
      shade(rect, palette.strip, undefined);
      labels.push({
        text,
        bounds: toCss({ x: rect.x, y: rect.y, width: rect.width, height: headerHeight }),
        node: layoutNode,
      });
      const inset = Math.max(1, Math.round(2 * scale));
      content = {
        x: rect.x + inset,
        y: rect.y + headerHeight,
        width: rect.width - 2 * inset,
        height: rect.height - headerHeight - inset,
      };
    }

    // The parent cushion shows through wherever children are sub-pixel or rounding leaves gaps.
    shade(content, palette.directory, own);
    if (content.width >= 3 && content.height >= 3) {
      const children = visibleTreemapChildren(
        layoutNode.children,
        layoutNode.physicalBytes,
        CANVAS_MICRO_RATIO,
      );
      const childRidge = depth === 0 ? ridge : ridge * FALLOFF;
      for (const child of squarify(children, content)) {
        draw(child.node, child.bounds, childRidge, own, depth + 1, [
          ...(depth === 0 ? [] : layoutTrail),
          child.node,
        ]);
      }
    }

    if (!headed && depth > 0) {
      const factor = FRAME_FACTORS[Math.min(depth, FRAME_FACTORS.length) - 1] ?? 0.8;
      const thickness =
        depth === 1
          ? Math.round(2 * scale)
          : depth === 2
            ? Math.max(1, Math.round(scale))
            : rect.width > 28 && rect.height > 28
              ? 1
              : 0;
      frame(rect, thickness, factor);
    }
  };

  if (width > 0 && height > 0) {
    draw(root, { x: 0, y: 0, width, height }, BASE_HEIGHT, FLAT, 0, []);
  }
  return { width, height, scale, ops, tiles, labels };
}

/**
 * Paints the scene into RGBA pixels. Only the last shade op over a pixel is visible, so owners
 * are resolved first with cheap integer fills and the cushion shader then runs once per pixel.
 */
export function paintTreemap(scene: TreemapScene, background: Rgb): Uint8ClampedArray<ArrayBuffer> {
  const { width, height, ops } = scene;
  const pixels = new Uint8ClampedArray(width * height * 4);
  const owner = new Int32Array(width * height).fill(-1);
  for (let index = 0; index < ops.length; index += 1) {
    const op = ops[index];
    if (op === undefined || op.type !== 'shade') continue;
    const x0 = Math.max(0, op.x0);
    const x1 = Math.min(width, op.x1);
    for (let y = Math.max(0, op.y0); y < Math.min(height, op.y1); y += 1) {
      owner.fill(index, y * width + x0, y * width + x1);
    }
  }

  for (let y = 0; y < height; y += 1) {
    for (let x = 0; x < width; x += 1) {
      const pixel = y * width + x;
      const op = ops[owner[pixel] ?? -1];
      let r = background[0];
      let g = background[1];
      let b = background[2];
      if (op !== undefined && op.type === 'shade') {
        const brightness =
          op.surface === undefined ? 1 : cushionBrightness(op.surface, x + 0.5, y + 0.5);
        r = op.rgb[0] * brightness;
        g = op.rgb[1] * brightness;
        b = op.rgb[2] * brightness;
      }
      const offset = pixel * 4;
      pixels[offset] = r;
      pixels[offset + 1] = g;
      pixels[offset + 2] = b;
      pixels[offset + 3] = 255;
    }
  }

  for (let index = 0; index < ops.length; index += 1) {
    const op = ops[index];
    if (op === undefined || op.type !== 'frame') continue;
    const x0 = Math.max(0, op.x0);
    const x1 = Math.min(width, op.x1);
    const darken = (from: number, to: number, y: number) => {
      for (let x = Math.max(x0, from); x < Math.min(x1, to); x += 1) {
        const pixel = y * width + x;
        if ((owner[pixel] ?? -1) > index) continue;
        const offset = pixel * 4;
        pixels[offset] = (pixels[offset] ?? 0) * op.factor;
        pixels[offset + 1] = (pixels[offset + 1] ?? 0) * op.factor;
        pixels[offset + 2] = (pixels[offset + 2] ?? 0) * op.factor;
      }
    };
    for (let y = Math.max(0, op.y0); y < Math.min(height, op.y1); y += 1) {
      if (y < op.y0 + op.thickness || y >= op.y1 - op.thickness) {
        darken(x0, x1, y);
      } else {
        darken(op.x0, op.x0 + op.thickness, y);
        darken(op.x1 - op.thickness, op.x1, y);
      }
    }
  }
  return pixels;
}

/** Deepest tile under a CSS-pixel point. */
export function hitTestTreemap(scene: TreemapScene, x: number, y: number): TreemapTile | undefined {
  for (let index = scene.tiles.length - 1; index >= 0; index -= 1) {
    const tile = scene.tiles[index];
    if (tile === undefined) continue;
    const { bounds } = tile;
    if (
      x >= bounds.x &&
      y >= bounds.y &&
      x < bounds.x + bounds.width &&
      y < bounds.y + bounds.height
    ) {
      return tile;
    }
  }
  return undefined;
}
