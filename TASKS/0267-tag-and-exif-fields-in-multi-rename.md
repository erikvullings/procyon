# 0267 Tag and EXIF fields in multi-rename

Status: open
Priority: medium
Subsystem: backend, frontend
Depends on: 0072, 0149

## Context
Multi-rename (0072) works on names, dates and counters. Photo and music collections need tokens from metadata, e.g. `{artist} - {title}` or `{exif:date-taken:yyyy-MM-dd}`.

## Acceptance Criteria
- Rename rules accept metadata tokens (audio tags: artist, album, title, track, year; image EXIF: date taken, camera, dimensions); live preview shows resolved names and flags files missing the field.
- Unsafe characters are sanitized per platform; collisions are detected before apply, as today.
- Saved presets (0149) store the tokens.

## Implementation Notes
- Backend resolves tokens (the frontend only supplies the pattern) so the same result is produced in browser and Tauri hosts.

## Agent Notes
- Created 2026-10-18 from the media/playlist feature discussion. No work started.
