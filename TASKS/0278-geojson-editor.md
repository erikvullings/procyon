# 0278 Simple GeoJSON editor (geojson.io-style)

Status: open
Priority: medium
Subsystem: frontend
Depends on: 0277, 0099

## Context
A lightweight editor similar to geojson.io: draw and modify features on the map, edit properties in a table, and see or edit the raw JSON side by side.

## Acceptance Criteria
- Drawing tools for point, line, polygon, rectangle and circle (as polygon); select, move, add/delete vertices, delete and duplicate features; undo/redo.
- Properties table per feature (add/rename/delete keys, typed values) and a raw JSON pane kept in sync both ways, with live validation errors.
- Save and Save As go through the existing editor save path (0099) with the same overwrite safeguards; unsaved-changes prompts; large-file limits as in 0277 with the editor disabled above them.
- Import by pasting or dropping GeoJSON text; export as GeoJSON (and as a follow-up: GPX/KML/CSV).
- Keyboard accessible where feasible (vertex nudging, feature list navigation); documented limits for screen readers on the map.

## Implementation Notes
- Use an established MapLibre drawing plugin (for example `terra-draw` or `maplibre-gl-draw`) rather than writing editing geometry from scratch; check its license and bundle size.
- Keep state in the Meiosis tree (ADR 0007); the editor works on a typed FeatureCollection model, with the text editor as another view.

## Agent Notes
- Created 2026-10-18 from the image-metadata/map/GeoJSON discussion. No work started.
