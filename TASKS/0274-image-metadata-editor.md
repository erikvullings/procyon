# 0274 Edit image metadata (EXIF, IPTC, XMP)

Status: open
Priority: medium
Subsystem: backend, frontend
Depends on: 0273, 0035

## Context
Viewing metadata (0273) is not enough: users want to fix the date taken, add or correct GPS coordinates, set title/description/keywords/copyright, and strip private metadata (location, camera serial) before sharing.

## Acceptance Criteria
- Edit dialog for common fields (date taken, GPS, title, description, keywords, rating, copyright/author, orientation) plus 'Remove all metadata' and 'Remove location only'; multi-selection applies shared fields to all files.
- Writes happen in the Rust operation engine as cancellable jobs: write to a temp file and atomically replace, preserving image data byte-for-byte (lossless, no re-encode) and unrelated tags; optional backup copy; undo support (0160) where feasible.
- Supported formats are explicit (at least JPEG and TIFF; PNG/WebP/HEIC as the chosen writer allows); unsupported ones are disabled with a reason.
- Remote/archive entries follow the same safety rules as other mutations; tests only in temp roots.

## Implementation Notes
- Candidate approach: XMP sidecar (`.xmp`) as an alternative for formats that cannot be safely written in place; make this a user choice.
- Date shifting for batches ('camera clock was 1 h off') is a natural addition; coordinate with multi-rename tokens (0267).

## Agent Notes
- Created 2026-10-18 from the image-metadata/map/GeoJSON discussion. No work started.
