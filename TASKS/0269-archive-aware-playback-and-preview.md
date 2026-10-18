# 0269 Archive-aware playback and preview

Status: open
Priority: low
Subsystem: backend, frontend
Depends on: 0076, 0260

## Context
Media inside archives (zip, tar, …) should play and preview without extracting first.

## Acceptance Criteria
- Audio/video/image entries inside an archive play or preview through the streaming path (0260) with seeking where the archive format allows (stored entries) and a bounded temp extraction otherwise.
- Temp data is cleaned up; encrypted archives prompt for the password as today.
- Queue (0262) accepts archive entries.

## Implementation Notes
- Compressed entries cannot seek cheaply; document the fallback and size limit.

## Agent Notes
- Created 2026-10-18 from the media/playlist feature discussion. No work started.
