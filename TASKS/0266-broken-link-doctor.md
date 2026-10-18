# 0266 Broken-link doctor

Status: open
Priority: low
Subsystem: backend, frontend
Depends on: 0068, 0168

## Context
Dead targets accumulate in playlists, symlinks, Windows shortcuts and Markdown links. A single tool should find them and suggest fixes.

## Acceptance Criteria
- Scan a folder tree for broken symlinks, `.lnk`/shortcut targets, playlist entries (0261) and relative Markdown/HTML links; results as a virtual folder with the reason per item.
- 'Find replacement' action searches by name (and size/checksum when known) and offers candidates; applying a fix is an explicit, undoable operation (0160).
- Never follows symlinks outside the scanned root; cancellable.

## Implementation Notes
- Link extraction for Markdown/HTML should be a small typed module with tests; do not execute or fetch anything.

## Agent Notes
- Created 2026-10-18 from the media/playlist feature discussion. No work started.
