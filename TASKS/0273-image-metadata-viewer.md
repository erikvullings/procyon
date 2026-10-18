# 0273 Full image metadata viewer (EXIF, IPTC, XMP)

Status: open
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
- Created 2026-10-18 from the image-metadata/map/GeoJSON discussion. No work started.
