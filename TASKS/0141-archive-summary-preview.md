# 0141 Archive summary preview

Status: open
Priority: low
Owner: unassigned
Agent: unassigned
Area: frontend
Depends on: 0071

## Context

Split out of [0071](0071-file-preview-architecture.md) in the 2026-08-15 re-triage, when that
task's PDF/CBR/EPUB/metadata-panel/folder-size pieces landed but this one didn't. Archives
(`.zip`/`.tar`/`.7z`/`.cbz`/`.cbr`/etc.) are already fully navigable as directories via task 0076's
archive provider — pressing F3 (`core.view`) on an archive today either opens it as a
directory-like listing (via the existing navigation path) or, for `.cbz`/`.cbr` specifically,
opens the comic-page renderer (task 0071's own PDF/CBR work). There is currently no dedicated
"summary" view of an archive *as a file* - total entry count, total uncompressed size, compression
ratio, format - the kind of at-a-glance info macOS Quick Look or 7-Zip's properties panel shows
without having to browse in.

## Acceptance Criteria

- Pressing F3 on a non-comic archive file (`.zip`, `.tar`, `.tar.gz`, `.7z`, `.rar`, etc. - the
  same extension list as `frontend/src/features/navigation/archive-location.ts`'s
  `ARCHIVE_SUFFIXES`, minus `.cbz`/`.cbr`/`.epub` which already have dedicated renderers) shows a
  new `PreviewKind: 'archiveSummary'` in the F3 viewer instead of falling through to the OS-open
  fallback or a directory-like listing.
- Summary includes: format (zip/tar/7z/rar/etc., from the same content-sniffed detection
  `crates/fm-archive/src/lib.rs`'s `detect_format` already does - don't re-derive from extension),
  total entry count (files vs. directories), total uncompressed size, and compressed size /
  compression ratio where the format exposes it cheaply (zip/7z do; plain tar does not, since it's
  uncompressed by definition - report "N/A" rather than a fake 1:1 ratio).
- Computing the summary walks the archive's directory tree the same way task 0071's
  `core.calculateFolderSize` walks a real directory (reuse `crates/fm-application/src/
  folder_size.rs`'s walker against the archive's `archive://...!/ ` root location - it's already
  provider-agnostic) rather than a bespoke archive-specific walk.
- Large archives (many thousands of entries) don't block the UI or make F3 feel unresponsive -
  same "show a loading state, allow cancellation via leaving the viewer" pattern as every other F3
  content kind.
- Tests: summary computation for zip/tar/7z fixtures with known entry counts/sizes, format
  detection reuse (no duplicated sniffing logic), UI rendering of the summary panel.

## Implementation Notes

- `content-preview.ts`'s `resolvePreviewKind` already special-cases `.cbz`/`.cbr` and `.epub`
  ahead of the generic archive case; add the new `archiveSummary` branch there, checked after those
  two so comics/EPUBs keep their dedicated renderers.
- Compression ratio and per-format compressed-size availability differ by format - check what
  `crates/fm-archive/src/lib.rs`'s listing already returns per entry (compressed vs. uncompressed
  size) before assuming a new backend field is needed; it may already be there and just unused.

## Agent Notes

- (none yet)
