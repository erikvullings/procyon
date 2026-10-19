import Supercluster from 'supercluster';

/** One point on the map. */
export interface MapMarker {
  readonly id: string;
  readonly latitude: number;
  readonly longitude: number;
  readonly title?: string;
  /** The marker may be dragged to a new position. */
  readonly draggable?: boolean;
}

export type Bounds = readonly [west: number, south: number, east: number, north: number];

/** What to draw for the current viewport: single markers, or groups with a count. */
export type ViewportItem =
  | { readonly kind: 'marker'; readonly key: string; readonly marker: MapMarker }
  | {
      readonly kind: 'cluster';
      readonly key: string;
      readonly clusterId: number;
      readonly count: number;
      readonly latitude: number;
      readonly longitude: number;
    };

interface PointProperties {
  readonly index: number;
}

/** Clusters markers per zoom level so dense folders stay readable and fast. */
export class MarkerIndex {
  private readonly index: Supercluster<PointProperties>;
  /** Markers with usable coordinates; `PointProperties.index` points into this list. */
  private readonly markers: readonly MapMarker[];

  constructor(markers: readonly MapMarker[], maxZoom: number) {
    this.markers = markers.filter(
      (marker) => Number.isFinite(marker.latitude) && Number.isFinite(marker.longitude),
    );
    this.index = new Supercluster<PointProperties>({
      radius: 60,
      maxZoom: Math.max(1, maxZoom - 1),
    });
    this.index.load(
      this.markers.map((marker, index) => ({
        type: 'Feature' as const,
        properties: { index },
        geometry: { type: 'Point' as const, coordinates: [marker.longitude, marker.latitude] },
      })),
    );
  }

  /** Items to draw inside `bounds` at an integer zoom. */
  viewport(bounds: Bounds, zoom: number): readonly ViewportItem[] {
    const [west, south, east, north] = bounds;
    const items: ViewportItem[] = [];
    for (const feature of this.index.getClusters([west, south, east, north], Math.round(zoom))) {
      const [longitude, latitude] = feature.geometry.coordinates as [number, number];
      const properties = feature.properties;
      if ('cluster' in properties && properties.cluster) {
        items.push({
          kind: 'cluster',
          key: `cluster:${properties.cluster_id}`,
          clusterId: properties.cluster_id,
          count: properties.point_count,
          latitude,
          longitude,
        });
        continue;
      }
      const marker = this.markers[(properties as PointProperties).index];
      if (marker !== undefined) items.push({ kind: 'marker', key: `marker:${marker.id}`, marker });
    }
    return items;
  }

  /** The zoom level at which a cluster falls apart. */
  expansionZoom(clusterId: number): number {
    return this.index.getClusterExpansionZoom(clusterId);
  }
}

/** The smallest box containing every marker, or undefined without markers. */
export function boundsOf(markers: readonly MapMarker[]): Bounds | undefined {
  const valid = markers.filter(
    (marker) => Number.isFinite(marker.latitude) && Number.isFinite(marker.longitude),
  );
  if (valid.length === 0) return undefined;
  return [
    Math.min(...valid.map((marker) => marker.longitude)),
    Math.min(...valid.map((marker) => marker.latitude)),
    Math.max(...valid.map((marker) => marker.longitude)),
    Math.max(...valid.map((marker) => marker.latitude)),
  ];
}
