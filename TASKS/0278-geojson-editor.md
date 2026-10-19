# 0278 Simple GeoJSON editor (geojson.io-style)

Status: done
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
- Created 2026-10-18 from the image-metadata/map/GeoJSON discussion. 
- 2026-10-18 claude: Map editor built into the existing editor tab (0099), so Save/Save-As/overwrite-conflict/unsaved-changes prompts are the editor's own: for .geojson (and sniffed GeoJSON .json) a 'Map editor' header toggle replaces the CodeMirror view with GeoMapEditor, which reads/writes the editor text. Drawing via terra-draw 1.37 + maplibre adapter (MIT, lazy-imported only when a map editor opens): point, line, polygon, rectangle, circle (stored as polygon), select/move/resize, vertex drag, midpoint insertion, vertex delete, Delete key; own undo/redo (GeoEditHistory, 200 steps) plus duplicate (offset copy) and delete. Document model keeps parsed JSON, so unknown members (bbox, name, foreign members, feature ids) survive saving; output is re-serialised pretty-printed. Properties table: add/rename/delete keys and typed values (text/number/boolean/null/JSON) with validation. JSON tab: raw text, debounced live validation through new POST /api/v1/geo/parse (+Tauri parse_geo_text, client.parseGeoText) using the same Rust validator as the viewer; valid text updates the model, invalid text shows line/column or JSON path and leaves the model untouched. Import: paste dialog or drop a file/text on the map (features are appended; an empty document is replaced). Viewer gets an 'Edit' button for .geojson. Keyboard: feature list is a listbox with Arrow navigation; Alt+Arrow nudges the selected feature ~1 m (Shift ~10 m); map canvas itself is not screen-reader accessible (documented in-UI). Limits: editor disabled above 1.5 MB / 5000 features (text view remains); multi-geometries, 3D positions and geometry collections are drawn read-only (edit in JSON tab); export = Save As via editor path (GPX/KML/CSV export arrives with 0281); state lives in a session controller object, not the Meiosis tree (same pattern as other editor controllers). NOT verified: real terra-draw interaction/rendering (only mocked in jsdom), so drawing UX needs a manual pass in the browser/Tauri app.
