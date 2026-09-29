import { describe, expect, it } from 'vitest';
import type { DiskUsageNode } from '../../models';
import {
  addRidge,
  buildTreemapScene,
  cushionBrightness,
  hitTestTreemap,
  paintTreemap,
  type TreemapScene,
} from './cushion-treemap';
import { DEFAULT_TREEMAP_COLOURS, type Rgb } from './file-type-colours';

function file(name: string, bytes: number): DiskUsageNode {
  return {
    name,
    kind: 'file',
    location: { providerId: 'local', uri: `file:///tmp/${name}` },
    logicalBytes: bytes,
    physicalBytes: bytes,
    collapsed: false,
    children: [],
  };
}

function folder(name: string, children: DiskUsageNode[], collapsed = false): DiskUsageNode {
  const bytes = children.reduce((sum, child) => sum + child.physicalBytes, 0) || 10;
  return {
    name,
    kind: 'directory',
    location: { providerId: 'local', uri: `file:///tmp/${name}` },
    logicalBytes: bytes,
    physicalBytes: bytes,
    collapsed,
    children,
  };
}

const BACKGROUND: Rgb = [0, 0, 0];

function pixel(scene: TreemapScene, pixels: Uint8ClampedArray, x: number, y: number): number[] {
  const offset = (y * scene.width + x) * 4;
  return [...pixels.slice(offset, offset + 3)];
}

describe('cushion shading', () => {
  it('is neutral on a flat surface and lit from the top-left on a ridge', () => {
    expect(cushionBrightness({ ax2: 0, ax1: 0, ay2: 0, ay1: 0 }, 5, 5)).toBeCloseTo(1);
    const ridge = addRidge(
      { ax2: 0, ax1: 0, ay2: 0, ay1: 0 },
      { x: 0, y: 0, width: 100, height: 100 },
      30,
    );
    const topLeft = cushionBrightness(ridge, 10, 10);
    const centre = cushionBrightness(ridge, 50, 50);
    const bottomRight = cushionBrightness(ridge, 90, 90);
    expect(centre).toBeCloseTo(1);
    expect(topLeft).toBeGreaterThan(centre);
    expect(bottomRight).toBeLessThan(centre);
  });
});

describe('buildTreemapScene', () => {
  it('paints every pixel and lets children cover their parent', () => {
    const root = folder('root', [file('a.mp4', 60), file('b.zip', 40)]);
    const scene = buildTreemapScene(root, 20, 10, 1, DEFAULT_TREEMAP_COLOURS);
    const pixels = paintTreemap(scene, BACKGROUND);

    expect(scene.tiles.map((tile) => tile.node.name)).toEqual(['a.mp4', 'b.zip']);
    for (let index = 3; index < pixels.length; index += 4) expect(pixels[index]).toBe(255);
    const video = hitTestTreemap(scene, 1, 5);
    expect(video?.node.name).toBe('a.mp4');
    // The video tile is orange-dominant, not the directory grey or the black background.
    const [r, , b] = pixel(scene, pixels, 5, 5);
    expect(r).toBeGreaterThan(b ?? 0);
  });

  it('renders depth beyond three levels and hit-tests the deepest tile', () => {
    const deep = folder('l1', [
      folder('l2', [folder('l3', [folder('l4', [file('leaf.bin', 50)])])]),
    ]);
    const root = folder('root', [deep, file('other.txt', 50)]);
    const scene = buildTreemapScene(root, 400, 400, 1, DEFAULT_TREEMAP_COLOURS);
    const leaf = scene.tiles.find((tile) => tile.node.name === 'leaf.bin');

    expect(leaf).toBeDefined();
    expect(leaf?.trail.map((node) => node.name)).toEqual(['l1', 'l2', 'l3', 'l4', 'leaf.bin']);
    const centre = {
      x: (leaf?.bounds.x ?? 0) + (leaf?.bounds.width ?? 0) / 2,
      y: (leaf?.bounds.y ?? 0) + (leaf?.bounds.height ?? 0) / 2,
    };
    expect(hitTestTreemap(scene, centre.x, centre.y)?.node.name).toBe('leaf.bin');
  });

  it('collapses single-child directory chains into one labelled strip', () => {
    const inner = folder('inner', [file('x.bin', 70), file('y.bin', 30)]);
    const root = folder('root', [folder('outer', [inner]), file('z.txt', 1)]);
    const scene = buildTreemapScene(root, 600, 400, 1, DEFAULT_TREEMAP_COLOURS);

    expect(scene.labels[0]?.text).toBe('outer  ▸  inner');
    expect(scene.labels[0]?.node.name).toBe('inner');
    expect(scene.labels.map((label) => label.text)).not.toContain('x.bin');
  });

  it('labels folders, never files, and scales geometry to device pixels', () => {
    const cache = folder('model-cache', [file('sha256-deadbeef', 80), file('small.json', 20)]);
    const scene = buildTreemapScene(folder('root', [cache]), 300, 200, 2, DEFAULT_TREEMAP_COLOURS);

    expect(scene.width).toBe(600);
    expect(scene.height).toBe(400);
    expect(scene.labels.map((label) => label.text)).toEqual(['model-cache']);
    expect(scene.labels[0]?.bounds.width).toBeCloseTo(300);
  });

  it('treats collapsed directories as leaves', () => {
    const modules = folder('node_modules', [], true);
    const scene = buildTreemapScene(folder('root', [modules]), 50, 50, 1, DEFAULT_TREEMAP_COLOURS);

    expect(scene.tiles).toHaveLength(1);
    expect(scene.tiles[0]?.node).toBe(modules);
    expect(scene.labels).toHaveLength(0);
  });

  it('darkens frame edges only where no later tile paints over them', () => {
    const root = folder('root', [folder('d', [file('a.bin', 1)]), file('b.txt', 1)]);
    const scene = buildTreemapScene(root, 20, 10, 1, DEFAULT_TREEMAP_COLOURS);
    const frames = scene.ops.filter((op) => op.type === 'frame');
    expect(frames.length).toBeGreaterThan(0);

    const withoutFrames = paintTreemap(
      { ...scene, ops: scene.ops.filter((op) => op.type !== 'frame') },
      BACKGROUND,
    );
    const withFrames = paintTreemap(scene, BACKGROUND);
    const edge = pixel(scene, withFrames, 0, 0);
    const unframedEdge = pixel(scene, withoutFrames, 0, 0);
    expect(edge[0]).toBeLessThan(unframedEdge[0] ?? 0);
  });
});
