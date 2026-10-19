import type { GeoDocument } from '../../models';

/** A KML folder (layer) with how many features it holds directly. */
export interface GeoLayer {
  /** Full path, for example `Trip / Day 1`. */
  readonly path: string;
  readonly name: string;
  readonly depth: number;
  readonly count: number;
}

const SEPARATOR = ' / ';

function folderOf(properties: unknown): string {
  const folder = (properties as { folder?: unknown } | null)?.folder;
  return typeof folder === 'string' ? folder : '';
}

/** Folders of a document in tree order, each with its direct feature count. */
export function layersOf(document: GeoDocument): readonly GeoLayer[] {
  const counts = new Map<string, number>();
  for (const feature of document.collection.features) {
    const folder = folderOf(feature.properties);
    if (folder === '') continue;
    const parts = folder.split(SEPARATOR);
    parts.forEach((_, index) => {
      const path = parts.slice(0, index + 1).join(SEPARATOR);
      counts.set(path, (counts.get(path) ?? 0) + (index === parts.length - 1 ? 1 : 0));
    });
  }
  return [...counts.entries()]
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([path, count]) => {
      const parts = path.split(SEPARATOR);
      return { path, name: parts.at(-1) ?? path, depth: parts.length - 1, count };
    });
}

/** Whether a feature's folder is hidden: it, or any folder above it, is switched off. */
export function isHidden(folder: string, hidden: ReadonlySet<string>): boolean {
  if (folder === '' || hidden.size === 0) return false;
  const parts = folder.split(SEPARATOR);
  return parts.some((_, index) => hidden.has(parts.slice(0, index + 1).join(SEPARATOR)));
}

export interface VisibleFeatures {
  readonly collection: GeoJSON.FeatureCollection<GeoJSON.Geometry | null>;
  /** Position in the original collection of each visible feature. */
  readonly originalIndexes: readonly number[];
}

/** The features whose layers are switched on, remembering where each came from. */
export function visibleFeatures(
  document: GeoDocument,
  hidden: ReadonlySet<string>,
): VisibleFeatures {
  if (hidden.size === 0) {
    return {
      collection: document.collection,
      originalIndexes: document.collection.features.map((_, index) => index),
    };
  }
  const features: GeoJSON.Feature<GeoJSON.Geometry | null>[] = [];
  const originalIndexes: number[] = [];
  document.collection.features.forEach((feature, index) => {
    if (!isHidden(folderOf(feature.properties), hidden)) {
      features.push(feature);
      originalIndexes.push(index);
    }
  });
  return { collection: { type: 'FeatureCollection', features }, originalIndexes };
}
