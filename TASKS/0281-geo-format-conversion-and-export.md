# 0281 Convert and export between GeoJSON, GPX, KML/KMZ and CSV

Status: open
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
- Created 2026-10-18 from the image-metadata/map/GeoJSON discussion. No work started.
