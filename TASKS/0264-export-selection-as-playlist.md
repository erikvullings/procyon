# 0264 Export selection or basket as a playlist

Status: done
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
- Created 2026-10-18 from the media/playlist feature discussion. 
- 2026-10-18 claude: Backend: createFile operation gained optional initialContent (StartOperationRequestDto.initial_content; fm-application operation_planner CreateFileExecutor writes it; refuses to overwrite an existing file, fails the operation visibly) - tests in crates/fm-application/tests/create_file_operation.rs. Frontend: playlist-writer.ts (M3U8 with #EXTM3U/EXTINF UTF-8, PLS, XSPF; relative paths only when every track shares the playlist's provider root, else absolute with a toast), parser/resolver updated so XSPF relative locations and same-scheme remote-provider URIs round-trip; round-trip tests for all formats x styles. UI: client.savePlaylist action (context menu on selection, palette; on the basket/virtual pane with nothing selected it takes every entry) + SavePlaylistDialog (name, format, path style); destination = active writable folder, else the other pane. Saved via OperationsController.createTextFile. Durations are not known when exporting from file entries (written as -1). Existing-name conflicts surface as a failed operation toast rather than the conflict dialog (createFile has no conflict prompt).
