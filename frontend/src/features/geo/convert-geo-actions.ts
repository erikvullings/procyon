import type { EntrySummary } from '../../models';

/** Opens "Convert geographic file..." for the selected files (task 0281). */
export const CONVERT_GEO_ACTION_ID = 'client.convertGeo';

/** Formats the converter reads; plain `.json` is excluded because it needs content sniffing. */
const READABLE_EXTENSIONS: ReadonlySet<string> = new Set(['geojson', 'gpx', 'kml', 'kmz']);

export function isConvertibleGeoFile(entry: EntrySummary): boolean {
  if (entry.kind !== 'file') return false;
  const extension = (entry.extension ?? entry.name.split('.').pop() ?? '').toLowerCase();
  return READABLE_EXTENSIONS.has(extension);
}
