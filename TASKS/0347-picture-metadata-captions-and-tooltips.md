# 0347 Picture metadata under thumbnails and in hover tooltips

Status: open
Priority: medium
Subsystem: backend, frontend, settings
Depends on: 0134, 0273

## Context
Directory Opus offers (a) extra lines of picture metadata under each thumbnail in thumbnail view (name, size, dimensions + bit depth, e.g. `4.47 MB (4000x2672x24)`), (b) a detailed list-style tile beside the thumbnail (name, type, dimensions, size), and (c) a hover tooltip with a larger preview and a two-column table of EXIF values (size, dimensions, modified, date taken, camera make/model, aperture, shutter, ISO, exposure bias/time, white balance, focal length, metering mode, flash, ...). Procyon has full metadata reading (0273) but grid tiles show only the file name.

## Acceptance Criteria
- Grid tile caption: configurable extra lines under the file name for images, taken from a chosen field list (default: file size + dimensions; options include date taken, camera model, exposure summary, rating). Lines truncate with ellipsis; tile height stays uniform so the virtualized grid math (0134) holds.
- Tooltip: hovering a tile (after a short delay, dismissed on scroll/move/Escape, never in the way of drag and drop) shows file name + type line, larger thumbnail, and a two-column table of EXIF fields present in the file; empty fields omitted. Also available on the table view's name cell. Keyboard-reachable equivalent (e.g. the cursor entry on a shortcut) so it is not hover-only.
- Field set chosen in Settings (caption fields, tooltip fields, tooltip on/off, delay); sensible defaults, no config required.
- Metadata is fetched lazily for visible/hovered entries only through one batched, cancellable call with a bounded read (header bytes only, no full-file decode), cached per file by content key; remote providers degrade gracefully (caption shows what the listing already knows: size, modified; tooltip shows "metadata unavailable").
- Non-image types reuse the machinery where cheap: audio (artist, album, duration, bitrate), video (resolution, duration, codec). Missing fields never render as blank labels.
- Values localized (units, dates) via the existing i18n; all new strings in every locale file.
- Tests: field formatting and omission of empties, batching/cancel/cache, caption does not change tile height, tooltip delay/dismiss behaviour, settings persistence.

## Implementation Notes
- Reuse 0273's `ImageMetadataDto` and its reader in `fm-metadata`; add a cheap summary endpoint (dimensions, bit depth, selected EXIF) rather than calling the full viewer read per tile. Regenerate OpenAPI/Orval client via `pnpm api:export` / `api:generate`, never hand-edit.
- Frontend: `directory-grid.ts` `renderTile` for captions; a new tooltip component owned by the pane/directory-table feature (no shared state in the tree except settings; hover state is local). Consider a metadata loader mirroring `ThumbnailLoader`.
- Coordinate with 0346 (album caption lines such as artist/year) so both use one caption-field registry.
- Directory Opus precedents: thumbnail captions ("Fields displayed under thumbnails"), tile mode, and tooltip field list are all user-configurable; keep ours smaller.

## Agent Notes
- Created 2026-11-23 from the Directory Opus comparison screenshots (captions under thumbnails, tile mode with details, EXIF tooltip). Not started.
