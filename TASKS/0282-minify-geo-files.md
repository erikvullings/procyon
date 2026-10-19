# 0282 Minify GeoJSON, GPX and KML files

Status: done
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
- Created 2026-10-18 from the image-metadata/map/GeoJSON discussion. 
- 2026-10-18 claude: fm-geo minify + transform. Two paths: whitespace-only is a LOSSLESS text pass (JSON: exact key order/number formatting kept; XML: indentation and comments removed, text/attributes untouched; KMZ repacked at deflate level 9 with .kml entries compacted, images byte-for-byte). Any other option works on the feature model: coordinate precision (elevation to 1 decimal), Douglas-Peucker simplification in metres for lines and polygon rings (per-point times/heart-rate arrays follow the kept points; a ring is only simplified if it does not start crossing itself, else kept and reported), drop elevation / timestamps / heart rates+custom properties, remove repeated points, empty and duplicate features. Statistics: features, points, summed line length before/after (GPX distance change shown as %), rings kept. Operation engine: TransformGeo gains minify/in_place/backup; new file name.min.ext (conflict flow as for conversion, undo removes it) or in-place replacement with .orig backup via the image-metadata temp+commit path (undo = RestoreContent; without backup undo is unavailable and the UI warns). POST /api/v1/geo/minify/preview returns sizes, stats, notes and the minified FeatureCollection (<=20k features) + Tauri command. Frontend: 'Minify geographic file...' action/context menu/viewer button -> MinifyGeoDialog with presets (whitespace/web/share/custom), options, debounced live size estimate, point and length change, notes, before/after map overlay (grey original, blue result; new MapView comparison layer), output choice (new file vs replace + backup toggle); GeoJSON editor gets Compact/Format buttons (text-only rewrite). Tests: Rust unit (precision, simplification tolerance, ring validity guard, aligned arrays, duplicates, lossless compaction) + operation integration (new file, in-place+undo, no-backup, invalid combos, preview). Limits: Visvalingam not offered; polygon holes vs outer ring and shared borders between polygons are not checked; ring validity check is O(n^2) up to 10k points; preview shows the first selected file only; KML/GPX model path re-writes the file so unsupported source elements are reported as lost.
