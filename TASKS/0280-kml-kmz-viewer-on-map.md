# 0280 View KML and KMZ on a map

Status: done
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
- Created 2026-10-18 from the image-metadata/map/GeoJSON discussion. 
- 2026-10-18 claude: fm-geo::parse_kml (streaming quick-xml, same XML safety as GPX: no entity expansion, ENTITY DOCTYPEs refused, line/column errors) and parse_kmz (zip read from memory, <=10k entries, 64 MiB KML cap, 1 MiB per image, 8 MiB / 64 images total; main file = doc.kml else shallowest .kml). Covers Placemark Point/LineString/LinearRing/Polygon with holes/MultiGeometry (merged into Multi* when homogeneous else GeometryCollection), Folder/Document nesting (feature property 'folder' = 'Trip / Day 1'), Style/StyleMap(normal)/inline styles resolved to simplestyle properties (LineStyle colour+width, PolyStyle fill/outline, IconStyle colour+scale; KML aabbggrr -> #rrggbb + opacity), descriptions (raw HTML kept), ExtendedData Data/SimpleData, TimeStamp/TimeSpan and altitudeMode kept as properties, 3D positions kept. Nothing named by the file is fetched: NetworkLink, external icons and external overlay images are listed (skipped/warnings); Model, gx:Track, Tour, Photo/ScreenOverlay and time animation are listed as skipped, never silently dropped. Images shipped in the KMZ are returned as data: URIs (resources) for icons (symbol layer) and GroundOverlay (rotated LatLonBox image source under the vector layers). Frontend: .kml/.kmz open in GeoView with folder layers (nested toggles, counts), icons and overlays, click popups and the property table with DESCRIPTION SANITISED through the existing preview sanitiser (scripts, images, forms removed; links inert text). Deviation: KMZ is read through the VFS provider of the .kmz file + in-memory zip (works for local/remote/in-archive files, never extracted to disk) rather than through fm-archive's path-based provider, which only addresses local files. Not done: no KML editing/export yet (0281); KML icon heading/hotspot, LabelStyle, region/LOD and schema typing ignored; fixtures are hand-written Google-Earth-style files.
