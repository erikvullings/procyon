# 0282 Minify GeoJSON, GPX and KML files

Status: open
Priority: low
Subsystem: backend, frontend
Depends on: 0281

## Context
Geo files from devices and web tools are large (excess coordinate precision, pretty-printing, vendor extensions, dense track points). A minify action produces much smaller files for sharing, web use or storage.

## Acceptance Criteria
- 'Minify…' dialog with options: remove whitespace/pretty-printing, limit coordinate precision (default 6 decimals for GeoJSON/KML, preset per use), simplify lines and polygons with a tolerance (Douglas-Peucker/Visvalingam, topology-preserving for polygons), drop elevation/time or vendor extensions, remove empty/duplicate points and features.
- Live size estimate and a before/after preview on the map (overlay) so the visual impact is visible; KMZ output is recompressed.
- Result is a new file or an in-place replacement with backup; operation-engine job with undo (0160) where in-place; the original is never modified without confirmation.
- Tests on precision, simplification tolerance, ring validity (no self-intersections introduced), and statistics (GPX distance change reported).

## Implementation Notes
- Reuse the conversion crate (0281); simplification is an algorithm module with property tests.
- GeoJSON minify without geometry changes (whitespace only) should also be a plain 'Format → compact' action in the editor (0278).

## Agent Notes
- Created 2026-10-18 from the image-metadata/map/GeoJSON discussion. No work started.
