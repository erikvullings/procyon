# 0274 Edit image metadata (EXIF, IPTC, XMP)

Status: done
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
- Created 2026-10-18 from the image-metadata/map/GeoJSON discussion. 
- 2026-10-18 claude: New OperationKind::EditImageMetadata (cancellable job, one item per file) run by image_metadata_edit.rs: read -> fm_metadata::apply_image_edit -> private sibling temp -> provider commit_copy (atomic replace, preserves mtime/permissions, same primitive as the text editor). Lossless: only metadata containers change, pixel data copied byte-for-byte (tested). EXIF/GPS via little_exif (unknown tags kept as raw); JPEG also keeps XMP (dc:title/description/rights/creator/subject, xmp:Rating, exif:GPS*) and an already-present IPTC APP13 consistent (IPTC rewritten as UTF-8 only when charset absent/UTF-8). Fields: date taken (+ per-file shift in minutes), GPS set/clear, description, copyright, author, orientation (all JPEG/TIFF/WebP) and title, keywords, rating (JPEG only); Remove location; Remove all (orientation kept, ICC kept). Formats explicit: JPEG, TIFF, WebP editable; PNG (writer stores EXIF in a non-standard zTXt chunk), HEIC/AVIF, RAW, GIF are reported unsupported -> per-file warning, nothing modified. Optional .orig backup; with backup the edit is undoable via new UndoAction::RestoreContent (verifies edited file unchanged + backup present, restores via temp+commit, removes backup); without backup undo is unavailable with a reason. Needs WRITE capability so archives/read-only providers are rejected at planning. Frontend: client.* action 'Edit image metadata...' (selection with editable images; context menu), EditImageMetadataDialog (blank = unchanged, per-field Remove, remove location/all, keep original), OperationsController.editImageMetadata. ImageMetadataDto.editableFields exposes the writer's capabilities. Also viewer EXIF ASCII values no longer quoted. Not done: XMP sidecar alternative and date-shift coordination with rename tokens; TIFFs lacking resolution tags fail in the writer (per-file failure); not exercised against real camera files/MakerNotes (synthetic fixtures only).
