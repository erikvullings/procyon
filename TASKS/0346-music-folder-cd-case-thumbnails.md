# 0346 Music folder view with CD-case album thumbnails

Status: open
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
- Created 2026-11-23 from the Directory Opus comparison screenshots. Not started.
