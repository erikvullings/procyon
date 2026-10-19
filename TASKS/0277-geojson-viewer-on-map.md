# 0277 View GeoJSON on a map

Status: done
Priority: medium
Subsystem: frontend, backend
Depends on: 0275, 0088

## Context
GeoJSON files (`.geojson`, `.json` containing a GeoJSON object, also TopoJSON as a possible follow-up) currently open as text or the structured data viewer. They should render on the map with their properties inspectable.

## Acceptance Criteria
- F3 viewer detects GeoJSON (by extension and content sniffing: FeatureCollection, Feature, Geometry) and shows it on the map with points, lines, polygons and multi-geometries, auto-fitted to bounds; a toggle switches to the raw text/tree view.
- Clicking a feature shows its properties in a table; optional styling from simplestyle-spec properties (`marker-color`, `stroke`, `fill`, ...).
- Bounded and safe for large files: size/feature limits with a clear message, streaming parse or simplification above the limit, never blocks the UI; invalid GeoJSON reports the validation error with position.
- Coordinate order and CRS rules follow RFC 7946 (WGS84 lon/lat); a warning is shown for legacy `crs` members or out-of-range coordinates.

## Implementation Notes
- Parsing and validation in a typed module with tests (bad geometry, empty collections, antimeridian, nested collections).
- The viewer should offer 'Edit' (0278) once that exists.

## Agent Notes
- Created 2026-10-18 from the image-metadata/map/GeoJSON discussion. 
- 2026-10-18 claude: New crate fm-geo: one GeoJSON-like feature model (GeoDocument) shared by all geo viewers/converters; GeoJSON reader/validator per RFC 7946 (FeatureCollection/Feature/bare geometry, all 7 geometry types, nested GeometryCollection depth-limited, empty geometries/collections, null properties normalised; legacy crs and out-of-range/unclosed ring data are warnings; syntax errors carry line+column, validation errors a JSON-pointer path like /features/3/geometry/coordinates/1/1). Limits: 64 MiB file cap, 50k features / 2M positions shown with a 'only N of M drawn' notice (parse runs on the backend, never the UI thread). Endpoint POST /api/v1/files/geo/read + Tauri read_geo_file + client.readGeoFile (all adapters); detection by .geojson or sniffed .json (ordinary JSON still opens as data). Frontend: viewer content kind 'geo' (GeoView) on the 0275 MapView, which gained vector layers (fill/line/circle from simplestyle properties marker-color/marker-size/fill/fill-opacity/stroke/stroke-width/stroke-opacity), click-to-inspect property table, highlight, fit-to-bounds; warnings, skipped elements and truncation shown beside the map; header toggle switches map <-> raw text/tree; parse errors show message + position with 'Show raw text'. Not done: no Edit button yet (0278); no vertex-level decimation above the limits (feature cap only); TopoJSON not supported; antimeridian data is drawn as given (bounds not wrapped); real map rendering not verifiable outside mocked MapLibre.
