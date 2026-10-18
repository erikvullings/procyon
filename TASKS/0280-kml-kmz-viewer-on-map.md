# 0280 View KML and KMZ on a map

Status: open
Priority: medium
Subsystem: frontend, backend
Depends on: 0275, 0088, 0076

## Context
KML (Google Earth, many GIS tools) and KMZ (zipped KML with icons and overlays) cannot be previewed at all today.

## Acceptance Criteria
- F3 viewer detects `.kml` and `.kmz` and renders placemarks, lines, polygons (with holes), folders (toggleable layers), styles (`Style`/`StyleMap`, colours, icons, line width) and descriptions (sanitized HTML popups).
- KMZ is opened through the archive provider (0076): the main `doc.kml` and referenced icons/images are read from the archive with bounded sizes; no extraction to disk.
- `NetworkLink`s and external resources are shown but never fetched automatically; ground overlays only render from local/in-archive images; unsupported features (3D models, tours, time animations) are listed as skipped instead of silently dropped.
- Safe XML handling (no external entities), size limits, and sanitized HTML (DOMPurify is already a dependency).

## Implementation Notes
- Typed KML subset parser with tests; consider a well-maintained library vs hand-written subset parser based on bundle size and security review.
- Altitude modes are ignored for 2D rendering but preserved on conversion where possible (0281).

## Agent Notes
- Created 2026-10-18 from the image-metadata/map/GeoJSON discussion. No work started.
