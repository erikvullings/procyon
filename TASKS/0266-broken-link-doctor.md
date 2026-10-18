# 0266 Broken-link doctor

Status: in_progress
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
- Created 2026-10-18 from the media/playlist feature discussion. 
- 2026-10-18 claude: PARTIAL - left in_progress. Done: scan job (crates/fm-application/src/link_doctor.rs + link_extract.rs: symlinks via resolve_symlink [local only], Windows .lnk targets [MS-SHLLINK LinkInfo parse; only checkable on Windows hosts], m3u/m3u8/pls/xspf entries, relative Markdown/HTML links skipping code/comments/URLs/anchors; symlinks are leaves so the walk never leaves the roots; cancellable; read-only, nothing fetched/executed), replacement search by name (ranked same size > exact case), REST /api/v1/link-doctor-scans + /link-doctor-replacements (roots-checked) + Tauri commands + client methods on all three adapters, panel UI (features/link-doctor) via palette action client.findBrokenLinks. Fix for a broken SYMLINK is undoable: old link goes to Trash (operation engine) then a relative symbolic link is created (createLink, undoable via 0160). NOT met: acceptance 'applying a fix is an explicit, undoable operation (0160)' for links inside text files (playlists, Markdown, HTML, .lnk shortcuts) - the panel only lists replacement candidates and opens the file in the editor (revision-guarded save, editor-local undo); 0160 has no undo action that restores edited file content, and adding one was out of scope. Next step if continued: a backend 'repoint link in file' operation with a content-backup UndoAction.
