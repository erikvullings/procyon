# 0281 Convert and export between GeoJSON, GPX, KML/KMZ and CSV

Status: done
Priority: medium
Subsystem: backend, frontend
Depends on: 0277, 0279, 0280

## Context
Users should be able to move data between the formats they receive and the formats their tools need, and export what they drew in the GeoJSON editor (0278).

## Acceptance Criteria
- 'Convert to…' / 'Export as…' for GeoJSON, GPX, KML, KMZ and CSV (points with lat/lon columns, WKT optional), from the viewer, the editor (0278) and the file context menu; multi-selection converts each file.
- Conversion runs as a cancellable job in the operation engine with conflict handling; output is a new file (never overwrites the source silently).
- A conversion report lists what could not be represented (for example KML styles in GPX, GPX extensions in GeoJSON, multi-geometries in CSV) and offers to continue or cancel before writing.
- The editor (0278) can open GPX/KML through the same conversion layer and export back; round-trip tests keep geometry and core properties lossless where the formats allow.

## Implementation Notes
- One internal feature model (GeoJSON-like) with per-format readers/writers in a Rust crate (new `fm-geo`?) so browser and Tauri hosts behave identically; the frontend viewers can share DTOs.
- Record in docs which properties map where (name, description, time, elevation, style).

## Agent Notes
- Created 2026-10-18 from the image-metadata/map/GeoJSON discussion. 
- 2026-10-18 claude: Conversion layer in fm-geo (one GeoJSON-like model, readers from 0277-0280, new writers): GeoJSON (pretty/compact), GPX 1.1 (waypoints, routes, tracks with segments, per-point time and heart rate via lossless read), KML 2.2 (Document, nested Folders in original order, simplestyle -> Style/LineStyle/PolyStyle/IconStyle, polygons with holes, Multi*/collections as MultiGeometry, ExtendedData, descriptions in CDATA, TimeStamp/TimeSpan), KMZ (deflated zip with doc.kml), CSV (points as latitude/longitude/altitude rows plus scalar property columns, optional WKT column keeping every geometry; formula-like text cells are prefixed with an apostrophe). Every writer returns notes for what the target cannot represent (areas in GPX, styles, folder structure, per-point times in KML/CSV, structured values, HTML descriptions, embedded images/overlays, skipped source elements); output is XML-escaped (hostile names tested). Round-trip tests: GPX->GeoJSON->GPX and KML->GeoJSON->KML keep geometry, names, times, heart rates, folders, stroke/fill and descriptions. Operation engine: new OperationKind::TransformGeo (cancellable, one item per file) writes a NEW file next to each source (or into a chosen folder) via private temp + commit_copy, never touching the source; name conflicts use the standard conflict flow (Ask -> conflict dialog, Skip, Overwrite, RenameNew -> 'name (copy N).ext'); losses are recorded as entry warnings in the history; undo removes created outputs (unavailable if a file was replaced). API: POST /api/v1/geo/convert/preview (per-file report before writing) and /geo/convert/text (editor export); Tauri equivalents. Frontend: 'Convert geographic file...' context-menu/palette action and viewer 'Convert...' button open ConvertGeoDialog which shows, per file, the output name and what would be lost and only then lets the user continue or cancel; the GeoJSON map editor gets 'Export...' (GeoJSON/GPX/KML/CSV +WKT, report shown first, saved as a new file next to the original). GPX/KML 'edit' = convert to GeoJSON, edit, export back (round-trip tested in Rust); there is no direct map-editor mode for GPX/KML, and KMZ is not offered for editor text export (binary). Renamed the reader's gpx kind property to gpxType so it survives GeoJSON conversion.
