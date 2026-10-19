/** A warning about data that is not represented faithfully (tasks 0277-0280). */
export interface GeoIssue {
  readonly path: string;
  readonly message: string;
}

/** Why a geographic file could not be read. */
export interface GeoError {
  readonly message: string;
  readonly line?: number | null;
  readonly column?: number | null;
  /** Location of a validation error inside the document, for example `/features/3/geometry`. */
  readonly path?: string | null;
}

/** One sample of a track's elevation, speed and heart-rate profile. */
export interface GeoProfilePoint {
  readonly distanceMeters: number;
  readonly longitude: number;
  readonly latitude: number;
  readonly elevation?: number | null;
  readonly speedMetersPerSecond?: number | null;
  readonly heartRate?: number | null;
}

/** Statistics of one recorded GPX track; values the file cannot support are absent. */
export interface GeoTrack {
  readonly name?: string | null;
  readonly featureIndex: number;
  readonly points: number;
  readonly distanceMeters: number;
  readonly elapsedSeconds?: number | null;
  readonly movingSeconds?: number | null;
  readonly ascentMeters?: number | null;
  readonly descentMeters?: number | null;
  readonly minElevation?: number | null;
  readonly maxElevation?: number | null;
  readonly averageSpeed?: number | null;
  readonly maxSpeed?: number | null;
  readonly startTime?: string | null;
  readonly endTime?: string | null;
  readonly profile: readonly GeoProfilePoint[];
}

/** An image draped over a rectangle of the map (KML ground overlay). */
export interface GeoOverlay {
  readonly name?: string | null;
  /** Path of the image in {@link GeoDocument.resources}. */
  readonly image: string;
  readonly north: number;
  readonly south: number;
  readonly east: number;
  readonly west: number;
  readonly rotation: number;
}

/** A geographic file as GeoJSON features, parsed by the backend. */
export interface GeoDocument {
  /** `geojson`, `gpx`, `kml` or `kmz`; empty when the file is not a geographic file. */
  readonly format: string;
  readonly collection: GeoJSON.FeatureCollection<GeoJSON.Geometry | null>;
  readonly totalFeatures: number;
  readonly displayedFeatures: number;
  readonly truncated: boolean;
  readonly bounds?: readonly [number, number, number, number] | null;
  readonly warnings: readonly GeoIssue[];
  readonly skipped: readonly string[];
  /** Statistics per recorded track (GPX). */
  readonly tracks?: readonly GeoTrack[];
  /** Images shipped inside a KMZ as `data:` URIs by path; features reference them by `_icon`. */
  readonly resources?: Readonly<Record<string, string>>;
  /** Ground overlays whose image is in `resources`. */
  readonly overlays?: readonly GeoOverlay[];
  readonly error?: GeoError | null;
}
