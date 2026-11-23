# 0346 Music folder view with CD-case album thumbnails

Status: done
Priority: medium
Subsystem: backend, frontend
Depends on: 0134

## Context
Directory Opus shows a folder of albums in thumbnail view with each album folder drawn as a jewel-case CD: cover art inside a case frame (dark spine strip on the left, soft border), name centred below in a link colour. Procyon's grid (0134) shows a generic folder icon for album folders and only thumbnails image/video/PDF/CBZ files, so a music library looks like a wall of identical folders.

Reference: Directory Opus "Nick Cave and the Bad Seeds" folder, one tile per album folder, cover art as the thumbnail, framed as a CD case.

## Acceptance Criteria
- Folder thumbnails: a folder whose direct children include cover art gets that image as its thumbnail. Lookup order: `cover`/`folder`/`front`/`album`/`albumart*` image files (jpg/jpeg/png/webp, case-insensitive), else embedded picture from the first audio file (ID3 APIC, MP4 `covr`, FLAC/Vorbis picture), else generic folder icon.
- "CD case" rendering in the grid: cover art square inside a jewel-case frame (spine strip on the left, thin border, subtle highlight), applied to folders that resolved cover art. Pure CSS/SVG over the normal thumbnail; the cache stores the plain downscaled cover, not the framed result.
- Style is a per-tab view setting (e.g. folder thumbnail style: off / plain / CD case), also automatic-default option "CD case when folder contains audio". Default keeps current behaviour for non-music folders.
- Audio files themselves show their embedded cover as thumbnail in grid and in the table icon column (same fallback chain as 0134).
- Lazy and budgeted like 0134: only visible tiles, bounded folder probing (one bounded listing per folder, max N bytes read per audio file for the picture), concurrency limit, content-keyed disk cache, probing never follows symlinks, works through any provider (local, SFTP, archives) with `READ`; unsupported/slow providers fall back to the icon, no stall.
- Tile caption shows the folder name (album); optional second line with artist/year from tags is covered by 0347's metadata caption machinery.
- Tests: cover-file selection order, embedded picture extraction per tag format (fixtures built in temp roots), budget enforcement, cache key stability, frontend test for CD-case class on folder tiles and fallback to icon.

## Implementation Notes
- Extend `crates/fm-application/src/thumbnails.rs` dispatch: new folder branch (list, pick cover) before the extension dispatch; extraction of embedded pictures belongs in `fm-metadata` (check what 0262/0265 already use for audio tags and reuse it, e.g. `lofty`, rather than adding another tag crate).
- Thumbnail endpoint (`GET /api/v1/thumbnails`) currently thumbnails files; decide whether folders reuse it with a `kind` hint or get entries flagged `hasCover` — avoid probing every folder in a 5k-entry directory just to learn it has no cover (probe only visible tiles; cache negative results with a short TTL / dir mtime key).
- Frontend: `frontend/src/features/directory-table/directory-grid.ts` `renderTile`; `thumbnail-loader.ts` `THUMBNAILABLE_EXTENSIONS` is extension-based and needs a folder path.
- Keep browser/Tauri/mock parity (mock client needs a fake cover fixture).

## Agent Notes
- Created 2026-11-23 from the Directory Opus comparison screenshots.
- 2026-11-23 claude: Implemented. Test seam agreed with the user: `ThumbnailService` only (8 new tests in `crates/fm-application/src/thumbnails.rs`: cover file, name priority, embedded picture in first track, audio file itself, no artwork -> NotFound, symlinked folder not probed). No other tests were added (user chose that one seam); the plumbing and frontend are covered only by existing tests plus tsc/lint and are not exercised in a browser or the Tauri app.
  **Backend:** `ThumbnailService` now treats a directory (`provider.inspect`, so symlinks are never followed) as a source: one bounded listing (512 entries), cover file by priority `cover` > `folder` > `front` > `album` > `albumart*` (jpg/jpeg/png/webp/etc., case-insensitive), else the embedded picture of the first of at most 3 audio tracks (by name). Embedded pictures are read from only the first 4 MiB of a file (`read_embedded_picture` in `fm-metadata/src/audio.rs`, lofty, front cover preferred, properties not parsed). Audio files themselves thumbnail from their own picture. The disk cache stays content-keyed on the picture bytes, so the plain cover is cached and the frame is CSS only. No HTTP route change: folders reuse `GET /api/v1/thumbnails`; "no art" is the existing 404 -> icon fallback.
  **Setting:** `FolderThumbnailStyle` (`off` | `plain` | `cdCase`, default `off`) added to `DirectoryViewConfiguration`, `DirectoryViewPatch`, per-folder view memory (`FolderViewMemory`, serde default) and the DTOs; OpenAPI and Orval client regenerated. Set from the grid's View menu ("Folders: icon / cover / CD case", i18n in all 8 locales). Folder tiles request thumbnails only when the style is not `off`.
  **Frontend:** `thumbnail-loader.ts` accepts directories and audio files for medium/large sizes; `directory-grid.ts` wraps folder covers in `.fm-cd-case` (spine strip, border, highlight; CSS in `directory-grid.css`).
  **Deviations from the acceptance criteria:** (1) no automatic "CD case when folder contains audio" default: it would need probing every folder; the style is an explicit per-folder choice (remembered per folder by 0270's view memory). (2) Audio covers are not shown in the table's 16px icon column, only in grid tiles, because each row would read up to 4 MiB of tags. (3) The artist/year caption line is left to 0347. (4) The mock client has no cover fixture, so the mock runtime shows no folder covers.
  **Watch for:** negative results (folders without art) are cached by the loader only for the grid TTL (60 s), so non-album folders are re-probed about once a minute while visible with a style switched on. M4A files with `moov` at the end of the file have their `covr` beyond the 4 MiB head and show no cover.
- 2026-11-23 claude: Review follow-up. (1) Folder listing now pages up to 2048 entries (512 per page) instead of one page. (2) Folder and audio probes run under a 10 s timeout and fall back to the icon, so a slow SFTP folder cannot hold a generation slot. (3) An audio file whose first 4 MiB hold no picture and were cut short is retried once with 16 MiB; new test `a_picture_larger_than_the_first_read_is_still_found` (4.3 MiB noise PNG in an ID3 tag). (4) Files with a known thumbnail extension skip the `inspect` round trip; a folder named like `Greatest Hits.mp3` therefore gets no cover (accepted). Still open: negative results are re-probed about once a minute (loader TTL); no paging regression test because listing order is provider-defined; M4A with `moov` past 16 MiB shows no cover.
