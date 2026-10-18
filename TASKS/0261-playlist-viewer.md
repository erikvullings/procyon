# 0261 Playlist files (M3U, M3U8, PLS, XSPF) as a track list

Status: done
Priority: medium
Subsystem: frontend, backend
Depends on: 0088, 0142

## Context
Playlist files currently open as plain text; nothing parses them. Showing them as a track list is cheap and useful on its own (and is the basis for playback).

## Acceptance Criteria
- F3/preview shows `.m3u`, `.m3u8`, `.pls`, `.xspf` as a track list with title, duration (when present) and resolved path; `#EXTINF` and encodings (UTF-8, Latin-1 for legacy `.m3u`) handled.
- Relative entries resolve against the playlist's own location through the VFS; missing targets are flagged, remote `http(s)` entries shown but never fetched without opt-in.
- Editor can still open the raw text (F4); parsing lives in a typed module with tests (including malformed files).

## Implementation Notes
- Parser could be a bundled plugin preview renderer (0142) or a core module; decide by whether playback (0262) needs it in core.
- Keep parsing in Rust if the backend also needs it for export (0264).

## Agent Notes
- Created 2026-10-18 from the media/playlist feature discussion. 
- 2026-10-18 claude: Parsing kept in TypeScript (frontend/src/features/playlist/playlist-parser.ts: M3U/M3U8 EXTINF, PLS, XSPF via DOMParser, UTF-8/UTF-16 BOM, Latin-1 fallback for .m3u only, 50k-entry cap, never throws; export in 0264 will reuse the same module). playlist-resolver.ts resolves relative/absolute/file:// entries against the playlist's own location (works inside archives/remote providers), checks existence via resolveLinkTarget (8 concurrent, progressive), classifies http(s)/other URL schemes as remote and never fetches them. F3 shows a virtualised track list (playlist-view.ts, content kind 'playlist'); F4 still edits raw text. i18n group 'playlist' (English fallback in other locales). No backend changes.
