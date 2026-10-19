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
  readonly error?: GeoError | null;
}
