import type { EntrySummary } from '../../models';

/** Extensions that always open on the map. `json` is added only after sniffing its content. */
export const GEO_EXTENSIONS: ReadonlySet<string> = new Set(['geojson', 'gpx', 'kml', 'kmz']);

/** Delimited text that may carry coordinates; it opens on the map only when columns are found. */
export const CSV_EXTENSIONS: ReadonlySet<string> = new Set(['csv', 'tsv']);

/** Largest CSV tried on the map without being asked; bigger ones open as a table first. */
export const CSV_AUTO_MAP_MAX_BYTES = 8 * 1024 * 1024;

export function isCsvEntry(entry: EntrySummary): boolean {
  return entry.kind === 'file' && CSV_EXTENSIONS.has(geoExtension(entry));
}

export function geoExtension(entry: EntrySummary): string {
  return (entry.extension ?? entry.name.split('.').pop() ?? '').toLowerCase();
}

/** Whether the viewer should try the map for this entry (plain `.json` is sniffed first). */
export function isGeoCandidate(entry: EntrySummary): boolean {
  if (entry.kind !== 'file') return false;
  const extension = geoExtension(entry);
  return GEO_EXTENSIONS.has(extension) || CSV_EXTENSIONS.has(extension) || extension === 'json';
}

/** True when the start of a `.json` file names a GeoJSON object type. */
export function looksLikeGeoJson(head: string): boolean {
  const text = head.replace(/^﻿/, '').trimStart();
  if (!text.startsWith('{')) return false;
  const match =
    /"type"\s*:\s*"(FeatureCollection|Feature|Point|MultiPoint|LineString|MultiLineString|Polygon|MultiPolygon|GeometryCollection)"/.exec(
      text.slice(0, 4096),
    );
  return match !== null;
}

/** A property value as shown in the feature table. */
export function propertyText(value: unknown): string {
  if (value === null || value === undefined) return '';
  if (typeof value === 'string') return value;
  if (typeof value === 'number' || typeof value === 'boolean') return String(value);
  return JSON.stringify(value);
}
