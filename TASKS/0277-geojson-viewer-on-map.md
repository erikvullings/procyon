# 0277 View GeoJSON on a map

Status: open
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
- Created 2026-10-18 from the image-metadata/map/GeoJSON discussion. No work started.
