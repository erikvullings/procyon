# 0273 Full image metadata viewer (EXIF, IPTC, XMP)

Status: done
Priority: medium
Subsystem: frontend, backend
Depends on: 0088

## Context
The F3 viewer parses only a few EXIF fields client-side (camera make/model, date taken, GPS via `exifr` in `frontend/src/features/preview/file-metadata.ts`) and links GPS to an external Google Maps URL. Users expect to see all metadata of an image: EXIF, IPTC, XMP, ICC profile, MakerNotes, and for other formats (PNG text chunks, HEIC, WebP, TIFF, RAW previews).

## Acceptance Criteria
- Metadata panel in the viewer lists all available tags grouped by namespace (EXIF, GPS, IPTC, XMP, ICC, file/format), with human-readable values and the raw value on demand; searchable and copyable.
- Works for JPEG, PNG, WebP, TIFF, HEIC/AVIF and common RAW containers where a parser exists; unsupported formats say so instead of showing an empty panel.
- Parsing is bounded (reads only the needed byte ranges, not whole large files) and runs for local, archive and remote entries through the VFS.
- GPS tags show coordinates plus an 'Open on map' action (0276) instead of the hardcoded Google Maps link.

## Implementation Notes
- Decide client parse (extend `exifr` with full segment options) vs backend parse (`fm-metadata`, e.g. a Rust EXIF/XMP crate) — the editor (0274) needs a writer anyway, so a single backend implementation is preferred for parity between hosts.
- Keep the existing Alt+Space info sub-panel working; this task enriches it.

## Agent Notes
- Created 2026-10-18 from the image-metadata/map/GeoJSON discussion. 
- 2026-10-18 claude: Backend parse (single implementation for 0274 writer + host parity): fm-metadata::image_tags reads EXIF/GPS/EXIF-thumbnail (kamadak-exif; JPEG, TIFF/RAW containers, PNG, WebP, HEIC/AVIF), IPTC-IIM (JPEG APP13), XMP (packet marker scan, works in any container), ICC header+description (JPEG APP2, PNG iCCP), PNG text chunks. MakerNotes shown as opaque byte count. Bounded: first 4 MiB through VFS provider (local/archive/remote). POST /api/v1/files/image-metadata + Tauri read_image_metadata + client.readImageMetadata on all 3 adapters. Unsupported format -> supported=false and the panel says so. Viewer: ImageMetadataView in the Alt+Space panel (grouped, filter, raw on demand, copy value/all); existing info fields kept. Google Maps link removed; GPS shows coordinates, 'Open on map' button appears when FileViewer gets onOpenOnMap - NOT wired yet, 0276 must pass it (no map exists before 0275). Limitations: RAW files whose EXIF lies beyond 4 MiB, GIF/legacy formats yield only what exists; XMP parsed as flat properties (nested structs not expanded); not verified against real camera files (synthetic fixtures).
