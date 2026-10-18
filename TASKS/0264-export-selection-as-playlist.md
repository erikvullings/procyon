# 0264 Export selection or basket as a playlist

Status: open
Priority: low
Subsystem: frontend, backend
Depends on: 0165, 0261

## Context
Turn the file basket or the current selection into an M3U8/PLS/XSPF playlist, with relative or absolute paths.

## Acceptance Criteria
- Action 'Save as playlist…' on the basket (0165) and on a selection; format and path style (relative to the playlist / absolute) selectable; M3U8 is UTF-8 with `#EXTM3U`.
- Created through the operation engine (conflict handling, no silent overwrite).
- Round-trip test: exported playlist parses back (0261) to the same entries.

## Implementation Notes
- Relative paths only when all entries share a provider with the target; otherwise warn and fall back to absolute.

## Agent Notes
- Created 2026-10-18 from the media/playlist feature discussion. No work started.
