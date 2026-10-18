# 0265 Duplicate and near-duplicate music finder

Status: open
Priority: low
Subsystem: backend, frontend
Depends on: 0077

## Context
Checksum duplicates (0077) miss the same track in different formats or bitrates. Combine tag metadata (artist, title, album, duration) with checksums to find likely duplicates.

## Acceptance Criteria
- Scan a folder tree for audio files; group exact duplicates (checksum) and likely duplicates (normalized artist+title+duration within tolerance) with a confidence label.
- Results open as a virtual folder with keep/remove helpers (prefer higher bitrate or lossless); deletion uses the normal operation engine and Trash.
- Cancellable, progress-reporting job; unreadable tags degrade gracefully.

## Implementation Notes
- Reuse `fm-metadata` for tags; related to perceptual image duplicates (0170) — share the grouping UI if possible.

## Agent Notes
- Created 2026-10-18 from the media/playlist feature discussion. No work started.
