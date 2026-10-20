import type { Location } from './location';

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

/** What converting one geographic file would lose (task 0281). */
export interface GeoConversionReport {
  readonly location: Location;
  readonly outputName?: string | null;
  readonly notes: readonly string[];
  readonly error?: string | null;
}

export type GeoTargetFormat = 'geojson' | 'gpx' | 'kml' | 'kmz' | 'csv';

/** Size reductions of a minification (task 0282); with none set only whitespace is removed. */
export interface GeoMinify {
  precision?: number;
  toleranceMeters?: number;
  dropElevation?: boolean;
  dropTime?: boolean;
  dropExtensions?: boolean;
  removeDuplicates?: boolean;
}

/** What a `transformGeo` operation does with each file. */
export interface GeoTransform {
  /** Output format; absent keeps each file's own format. */
  targetFormat?: GeoTargetFormat;
  csvWkt?: boolean;
  minify?: GeoMinify;
  /** Replace the source instead of writing a new file (reductions only). */
  inPlace?: boolean;
  /** With `inPlace`: keep the original as `<name>.orig`, which makes the job undoable. */
  backupOriginal?: boolean;
}

/** Sizes, statistics and the result of a minification preview (task 0282). */
export interface GeoMinifyPreview {
  readonly originalBytes: number;
  readonly minifiedBytes: number;
  readonly featuresBefore: number;
  readonly featuresAfter: number;
  readonly positionsBefore: number;
  readonly positionsAfter: number;
  readonly distanceBeforeMeters: number;
  readonly distanceAfterMeters: number;
  readonly notes: readonly string[];
  readonly collection?: GeoJSON.FeatureCollection<GeoJSON.Geometry | null> | null;
  readonly error?: string | null;
}

/** A geographic file as GeoJSON features, parsed by the backend. */
/** What the user chose for reading a CSV file as geographic data; omitted fields are detected. */
export interface CsvGeoOptions {
  readonly xColumn?: string;
  readonly yColumn?: string;
  readonly wktColumn?: string;
  /** Any EPSG code (`EPSG:28992`, `28992`) or a `+proj=...` definition. */
  readonly crs?: string;
}

/** How a CSV file was read, and what it offers for the "columns and coordinate system" dialog. */
export interface CsvGeoInfo {
  readonly headers: readonly string[];
  readonly delimiter: string;
  readonly xColumn?: string | null;
  readonly yColumn?: string | null;
  readonly wktColumn?: string | null;
  readonly crs?: string | null;
  readonly crsName?: string | null;
  readonly crsGuessed: boolean;
  readonly columnsDetected: boolean;
  readonly rows: number;
  readonly skippedRows: number;
}

export interface GeoDocument {
  /** `geojson`, `gpx`, `kml`, `kmz` or `csv`; empty when the file is not a geographic file. */
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
  /** How a CSV file was read; with only the columns when the file has no coordinates. */
  readonly csv?: CsvGeoInfo | null;
}
