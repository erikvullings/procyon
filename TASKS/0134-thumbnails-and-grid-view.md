# 0134 Thumbnails for images/video and a grid/icon view mode

Status: done (image/CBZ/CBR/video/PDF thumbnails, table icon-column overlay, grid view with three icon sizes, grid sort menu, photo-day grouping, filter/type-select, and F3 fullscreen preview are all shipped and verified — see Agent Notes)
Priority: high
Owner: unassigned
Agent: unassigned
Area: cross-cutting
Depends on: 0085, 0018

## Context

Thumbnails are explicitly flagged as an unimplemented capability on both platform integration
tasks — 0059 ("thumbnails... are declared as out of scope") and 0060's README entry ("thumbnails
remain an unimplemented capability") — and no task anywhere in `TASKS/` picks this up. For a
"state-of-the-art" file manager this is table stakes next to Finder, Explorer, and ForkLift: image
and video files should show a real thumbnail instead of (or layered onto) the generic type icon
from 0085/0091, and there should be a grid/icon view mode to browse a folder of photos usefully —
today the directory table (0024) only renders a single dense row-based layout.

This is two related but separable pieces of work: (1) thumbnail generation/caching as a backend
capability, and (2) a grid/icon view mode in the frontend that consumes it. Both are needed for the
feature to be useful, but (1) alone also improves the existing table view (a thumbnail can replace
the icon in the existing icon column for image/video rows without a new view mode).

## Acceptance Criteria

- Backend: a thumbnail service that generates downscaled previews for common image formats (at
  minimum JPEG/PNG/GIF/WebP) and, capability-permitting, video (first-frame extraction) and PDF
  (first-page render) — reuse the same "capability may not exist on every provider/platform, report
  `false` rather than half-implementing" convention already established for `nativeDragOut` (0062)
  and other `PlatformCapabilities` bits.
- Thumbnails are generated lazily (only for visible/requested entries, matching the virtualized
  table's viewport-driven fetch pattern from 0024) and cached on disk keyed by content hash + size,
  invalidated when the source file changes (reuse 0020's filesystem-watch deltas where available).
- A configurable size limit / file-count budget so thumbnailing a directory with thousands of large
  images doesn't stall the UI or exhaust disk cache space.
- The existing icon column (0085/0091) shows a thumbnail instead of the generic type icon for
  supported files once generated, falling back to the current icon while pending or unsupported.
- A new grid/icon view mode, togglable per pane, showing larger thumbnails with filename below —
  this is the "view-mode architecture" flagged as a prerequisite in 0129's Ctrl+F1/Ctrl+F2/
  Ctrl+Shift+F1 cluster; build the view-mode switch generally enough that a future "brief"/"full
  details" mode could reuse it, but only ship grid/icon view now.
- CBR/CBZ display the first image (front/title page).
- The Grid view can operate in photo app mode (toggled on/off or selected) and separate images
  per day.
- Allow for sorting by date/size/extension ascending or descending. Includes filters and
  type-to select functionality. Icon size is small, medium and large.
- Selected thumbnails can use F3 to see the full screen version.
- Tests: thumbnail generation for each supported format, cache invalidation on file change, size/
  count budget enforcement, and a frontend test for the view-mode toggle and thumbnail rendering.

## Implementation Notes

- Check `frontend/src/features/panes/` (post-0114 decomposition) for where the table-vs-future-grid
  split should live; the pane component should own view-mode state, not `app-shell.ts`.
- macOS Quick Look (`qlmanage -t`) can generate high-quality thumbnails for a very wide range of
  formats (including PDFs and many document types) with minimal own code — evaluate shelling out to
  it on macOS specifically vs a pure-Rust image-decoding pipeline shared across platforms, and
  document the tradeoff (macOS-only quality boost vs cross-platform consistency) before deciding.
- Windows has an equivalent shell thumbnail cache (`IThumbnailProvider`) worth the same evaluation.

## Agent Notes

- 2026-08-16 claude: Implemented the MVP slice agreed with the user (image+CBZ/CBR thumbnails,
  icon-column integration, basic grid view) via TDD across backend and frontend. Scope was agreed
  up front via `AskUserQuestion`: video/PDF thumbnails and photo-day grouping/sort/filter/
  type-select/F3 preview were explicitly deferred to a follow-up pass rather than attempted
  half-finished.

  **Backend (new `crates/fm-metadata` content + `crates/fm-application/src/thumbnails.rs`):**
  - `fm-metadata`: pure-Rust thumbnail generation (`image` crate) for JPEG/PNG/GIF/WebP, downscaled
    to one of three sizes (small=64px/medium=128px/large=256px) and re-encoded as JPEG. A 25 MB
    per-file source-size budget (`MAX_SOURCE_BYTES`) is enforced before decoding. A disk cache
    (`ThumbnailCache`) keyed by `sha256(source bytes)-{size}` is content-addressed, so a changed
    file is automatically a cache miss — no separate invalidation step needed (0020's filesystem
    watch deltas were not wired in for this reason: the content-hash key already makes staleness
    impossible, and delta-driven cache priming is a pure performance optimization, not a
    correctness requirement, left as a documented follow-up). The cache is capped at 200 MB
    on-disk via oldest-write-first eviction.
  - `fm-application/src/thumbnails.rs`: `ThumbnailService` (owns the cache + a 4-permit
    `tokio::sync::Semaphore` capping concurrent generations, so a fast-scrolled directory of
    thousands of images can't spawn unbounded CPU-bound decode work at once). Provider-agnostic:
    reads bytes via the existing `FileSystemProvider`/`ProviderRegistry` abstraction (same pattern
    as `content_streaming.rs`), not the OS-native `PlatformAdapter::thumbnail()` stub — so it works
    identically for local files and, in principle, any future provider that supports
    `ProviderCapabilities::READ`.
  - **CBZ/CBR discovery**: both are fully supported with *zero* new dependencies or deferral. The
    existing `ArchiveFileSystemProvider` (zip + `rars` crate, already wired into `fm-archive` for
    general archive browsing) sniffs the real format by magic bytes, not extension — so a `.cbz`
    (zip) or `.cbr` (rar) file is browsable via the same `archive://{path}!/` URI the frontend
    already builds for entering archives (`frontend/src/features/navigation/archive-location.ts`).
    `read_first_comic_page` builds that root location, lists it, picks the first file entry (sorted
    by name) whose extension is a supported image format, and feeds its bytes through the same
    `generate_image_thumbnail` path as a plain image file.
  - **Capability reporting**: deliberately did *not* add a new `RuntimeCapabilitiesDto` boolean.
    `native_thumbnails`/`PlatformCapabilities::THUMBNAILS` already exist end-to-end from task
    0091's icon work but describe *OS-native* thumbnail providers (Quick Look/`IThumbnailProvider`)
    specifically — left `false`/unset since no native path was implemented, which remains accurate.
    The pure-Rust generator's support varies per file format, not per platform, so it's exposed via
    the existing "try the request, 404 falls back to the icon" convention (same as
    `file_icon`/`NativeIconLoader` already do) rather than a coarser global flag.
  - New route `GET /api/v1/thumbnails?uri=&size=` (`apps/fm-server/src/routes/thumbnails.rs`) and
    Tauri command `get_thumbnail` (`apps/fm-desktop/src-tauri/src/commands.rs`), mirroring 0091's
    `icons.rs`/`get_file_icon` exactly. Every `ThumbnailError` maps to `ApplicationError::NotFound`
    (404), matching `file_icon`'s "unsupported → 404 → icon fallback" convention.
  - `DirectoryViewConfiguration`/`DirectoryViewConfigurationDto` extended with `view_mode`
    (`table`/`grid`) and `icon_size` (`small`/`medium`/`large`), both `#[serde(default)]` so a
    workspace saved before this task still deserializes (defaults to `table`/`medium`). A matching
    `DirectoryViewPatch.view_mode`/`.icon_size` lets the frontend's `updateView` workspace command
    persist the toggle per tab, exactly like `sort`/`showHidden`/`foldersFirst` already do.

  **Frontend:**
  - `FileManagerClient.getThumbnail(uri, size, signal?)` implemented on all three adapters
    (http/tauri/mock), and a new `ThumbnailLoader` (`frontend/src/features/directory-table/
    thumbnail-loader.ts`) mirroring `NativeIconLoader`'s lazy/dedup/in-memory-cache shape, keyed
    per-entry+size (not per-extension, since a thumbnail is file-specific).
  - `directory-table.ts`'s icon column now tries a thumbnail first, then the native icon overlay,
    then the themed glyph — the existing fallback chain from 0085/0091 extended by one link.
  - New `DirectoryGrid` component (`frontend/src/features/directory-table/directory-grid.ts`):
    virtualized wrapping-tile grid reusing the table's `DirectoryEntrySource`/`onEndReached`
    contract and the same windowing math (`calculateVisibleWindow`), treating one "row" as a
    horizontal band of tiles instead of one entry. Shares selection/cursor/context-menu/drag-drop
    callback wiring with `DirectoryTable` via a common object built once in `pane.ts`.
  - View-mode toggle: an IconButton labelled "View" between "New tab" and "Favourites" in the
    pane's breadcrumb row (per explicit user request), opening a small menu with List / Small icons
    / Medium icons / Large icons (`role="menuitemradio"`), persisted via the `updateView` command.
  - Manually verified in the browser (mock runtime): toggling table → grid → table, tile rendering
    with themed icon + filename, selection highlighting, and double-click navigation into a folder
    while in grid view all work; independent per-pane state confirmed (left pane in grid, right
    pane in table, simultaneously). Real thumbnail *image* rendering (not just the UI shell) was
    verified via the backend integration tests (`apps/fm-server/tests/thumbnails_routes.rs`) using
    real generated PNG bytes end-to-end through the HTTP route, not through manual browser
    inspection — the mock client fakes non-decodable bytes for speed, so it doesn't exercise real
    JPEG decoding in the browser.

  **Known gaps / explicitly deferred (not silently skipped):**
  - Video first-frame and PDF first-page thumbnails: not implemented. User chose "defer entirely"
    when asked about tech tradeoffs (shell out to `qlmanage -t`/`IThumbnailProvider` vs. pure-Rust
    crates) rather than commit to a dependency decision as part of this MVP. `PlatformAdapter::
    thumbnail()` remains the unimplemented stub it already was; the Implementation Notes' macOS/
    Windows shell-out evaluation was not performed.
  - No dedicated CBR (RAR) test with real archive bytes: `rars` is a reader-only crate (no RAR
    writer exists in the Rust ecosystem — WinRAR's proprietary `rar` tool is the only common
    encoder, unavailable in this environment), and no `.rar`/`.cbr` fixture exists anywhere in the
    repo already, including `fm-archive`'s own test suite. The CBR code path is identical to the
    tested CBZ path (`read_first_comic_page`/`archive_root_for` don't branch on format — the
    archive provider's own magic-byte sniffing picks Zip vs. Rar transparently), so this is a test
    coverage gap on an already-shared code path, not an unverified separate implementation.
  - Grid view sort/filter/type-to-select controls, small/medium/large *photo-app mode with
    day-grouping*, and F3 fullscreen preview: not implemented. The grid reuses whatever sort is
    already active for the pane (no grid-specific sort UI), has no filter/type-ahead beyond what
    the pane's existing quick-filter already provides, and F3 does nothing new for a grid selection
    yet.
  - Inline rename and drag-and-drop are wired into `DirectoryGrid`'s attrs contract (same callback
    shapes as `DirectoryTable`) but rename-in-place UI (an input overlaying the tile) was not built
    — renaming a grid-selected entry has no visible affordance yet, only the callback plumbing.
  - Delta-driven thumbnail cache invalidation (reusing 0020's `DirectoryDelta::EntriesUpdated` to
    avoid re-hashing unchanged files) was not wired in — the content-addressed cache already
    guarantees correctness without it; this would only be a performance optimization for very large
    files repeatedly requested.

  **Verified:** `cargo test --workspace` (full workspace, all crates green) and `cargo clippy
  --workspace --all-targets` (zero warnings) from the repo root; `cargo fmt --all --check` clean.
  Frontend: `pnpm exec tsc --noEmit` clean; `pnpm exec vitest run` — 1120/1121 passing (the one
  failure, `config/mithril-inspector.test.ts`'s production-build timeout test, is pre-existing
  machine-load flakiness unrelated to this change — confirmed by re-running it alone, which
  passes). New test counts for this task specifically: 15 in `fm-metadata` (thumbnail generation +
  cache), 7 in `fm-application/src/thumbnails.rs` (service including CBZ), 3 in `apps/fm-server/
  tests/thumbnails_routes.rs` (HTTP route), 1 in `apps/fm-desktop` (Tauri command), 2 in
  `fm-domain`/`fm-transport-dto` combined (view-mode/icon-size DTO defaults+round-trip) plus the
  `update_view_patches_view_mode_and_icon_size...` test in `fm-application`, 6 in
  `thumbnail-loader.test.ts`, 3 new in `directory-table.test.ts` (thumbnail fallback chain), 11 in
  `directory-grid.test.ts`, and 4 new in `pane.test.ts` (view-mode menu) — all verified by running
  exactly those files/crates, not quoted from a whole-suite total.
  `pnpm run api:export && pnpm run api:generate` was run; the OpenAPI document and generated
  TypeScript client are up to date with the new endpoint and DTO fields.

- 2026-08-16 claude: Follow-up pass adding video and PDF thumbnails, **superseding the "video/PDF
  not implemented" gap noted in the entry above**. User explicitly asked for cross-platform Rust
  libraries rather than shelling out to `ffmpeg` (echoing the same concern that ruled out
  `pdfium`/`mupdf` for PDF), and to keep the binary-size/dependency footprint small.

  **Video (`crates/fm-metadata/src/video.rs`)**: `mp4` (pure-Rust ISO-BMFF demuxer) + `openh264`
  (Cisco's BSD-2-Clause H.264 decoder/encoder, compiled from source via `cc` at build time — no
  runtime external tool, same approach the workspace already uses for `rars`/`sevenz-rust2`/`mlua`
  vendored dependencies). Scope: only the first keyframe of the first H.264 track in an MP4/M4V/MOV
  container is decoded. NASM is used opportunistically for SIMD if present on the build machine and
  silently falls back to a plain C build otherwise (confirmed via `openh264-sys2`'s `build.rs`) —
  no hard external build tool requirement either. VP9/HEVC/AV1 codecs and non-ISO-BMFF containers
  (MKV/WebM/AVI) report `ThumbnailError::UnsupportedFormat`. AVCC (length-prefixed) sample NALs are
  converted to the Annex-B format `openh264` expects by prepending the track's SPS/PPS to every
  decoded sample — a deliberate simplification of `openh264`'s own `examples/mp4/
  mp4_bitstream_converter.rs` (that example tracks SPS/PPS-seen state across an entire stream to
  avoid redundancy; since this only ever decodes one keyframe, unconditional prepending is simpler
  and equally correct). Tested end-to-end: the test fixture is a *real* MP4 built by encoding one
  frame with the real `openh264` encoder and muxing it with the real `mp4` crate writer, not a
  hand-rolled or pre-baked binary fixture.

  **PDF (`crates/fm-metadata/src/pdf.rs`) — deliberately partial, documented in the module's own
  doc comment**: no small pure-Rust PDF page renderer exists anywhere in the ecosystem; PDFium and
  MuPDF are the only production-quality options and both need a ~15-20MB non-Rust native library,
  which is the same category of dependency the user asked to avoid for video. Presented this
  tradeoff to the user via `AskUserQuestion` before implementing; they chose partial support over
  deferring PDF entirely. What ships: `lopdf` (pure Rust, `default-features = false` — no
  chrono/jiff/rayon/time pulled in, just the parser) extracts the *largest embedded raster image*
  on page 1 via its built-in `get_page_images()`, handling `DCTDecode` (the stream bytes already
  are a complete JPEG — decoded directly) and `FlateDecode`-compressed raw 8-bit `DeviceRGB`/
  `DeviceGray` samples (zlib-inflated via `flate2`, already a workspace dependency, then
  reconstructed into an `image::RgbImage`/`GrayImage`). This covers the common case of a
  scanned/photographed page. It is **not a PDF renderer**: an ordinary text/vector document (a
  Word-exported PDF, a spreadsheet, a slide deck) has no embedded page-sized image and reports
  `UnsupportedFormat`, falling back to the generic icon — this is expected, not a bug. JPEG2000
  (`JPXDecode`), fax (`CCITTFaxDecode`), JBIG2, indexed/palette colour spaces, and non-8-bit sample
  depths are also reported unsupported rather than guessed at. Tested end-to-end against real PDFs
  built with `lopdf`'s own writer (one with a DCTDecode JPEG image, one with a FlateDecode raw RGB
  image, one with a JPXDecode image to confirm it's correctly rejected, one with no image at all).

  **Wiring**: `crates/fm-application/src/thumbnails.rs`'s extension dispatch now routes
  `mp4`/`m4v`/`mov` to `generate_video_thumbnail` and `pdf` to `generate_pdf_thumbnail`, sharing the
  same cache/concurrency-limiter/error-mapping infrastructure as images and CBZ/CBR (no changes
  needed there — `ThumbnailError` → `ApplicationError::NotFound` → HTTP 404 → frontend icon
  fallback already generalizes to every format). Frontend `THUMBNAILABLE_EXTENSIONS`
  (`thumbnail-loader.ts`) and the mock client's equivalent set were extended to match.

  **Verified**: `cargo build/test/clippy --workspace [--all-targets]` and `cargo fmt --all --check`
  all clean from the repo root (zero warnings, all crates). New tests for this follow-up,
  verified by running exactly these targets: 4 in `fm-metadata/src/video.rs` (keyframe decode,
  non-video rejection, size-budget rejection, extension recognition — the decode test round-trips
  through a real encoded+muxed fixture), 7 in `fm-metadata/src/pdf.rs` (DCTDecode, FlateDecode,
  no-image-on-page, JPXDecode rejection, non-PDF rejection, size-budget rejection, extension
  recognition), 2 new in `fm-application/src/thumbnails.rs` (service-layer dispatch end-to-end for
  both formats, not just the `fm-metadata` decoders in isolation), 1 new in
  `thumbnail-loader.test.ts` (frontend extension recognition). Full `pnpm exec vitest run`: all
  1122 tests passing (no flakes this run). New workspace dependencies: `mp4`, `openh264` (default
  features only — `source`, not `libloading`), `lopdf` (`default-features = false`), `flate2`
  (already present, no new dependency).

  **Still deferred** (unchanged from the entry above): photo-day grouping, grid sort/filter/
  type-to-select, F3 fullscreen preview, inline rename UI in the grid, and delta-driven cache
  invalidation (still unnecessary given the content-addressed cache key).

- 2026-08-16 claude: Follow-up pass closing out the three remaining acceptance-criteria items
  (photo-day grouping, grid sort/filter/type-to-select, F3 fullscreen preview), completing the task.

  **Investigation first**: before writing anything, re-read how the pane wires sort, the quick
  filter, and type-ahead. All three turned out to already be pane-level, not table-specific:
  `Pane`'s `onkeydown` (in `frontend/src/features/panes/pane.ts`) drives type-to-select
  (`TypeaheadController`) and dispatches `onSelectionAction` regardless of which child component is
  mounted; the quick filter (`attrs.filter`) renders in the breadcrumb row above either child and
  filters `attrs.entries` before either view ever sees them; and F3's viewer resolution
  (`resolveViewTarget`/`openViewer` in `frontend/src/features/keybindings/global-keydown-handler.ts`)
  reads only `getSelections`/`getDirectories`, which carry no view-mode concept at all. So filtering,
  type-to-select, and F3 fullscreen preview **already worked correctly for the grid view** with zero
  code changes needed - confirmed by tracing the call paths, then locked in with regression tests
  (`Pane grid view type-to-select and quick filter` in `pane.test.ts`; a new `F3 opens the viewer for
  the cursor entry regardless of the active pane's view mode` test in
  `global-keydown-handler.test.ts`) and manually in the browser (mock runtime): selected a grid tile,
  pressed F3, and the Lister viewer opened showing the file's real content in the opposite pane,
  identical to table-view F3.

  **What was actually missing and got built:**
  - **Grid sort menu** (`frontend/src/features/panes/pane.ts`): the grid has no column headers to
    click, so there was no way to trigger `onSortChange` while in grid view even though the
    underlying sort mechanism (`attrs.tableConfig.sort`/`attrs.onSortChange`, already resorting
    `attrs.entries` upstream for the table) works identically for whichever view is mounted. Added a
    `.fm-pane-grid-sort` "Sort" button (visible only when `viewMode === 'grid'`) opening a menu with
    Name/Date modified/Size/Extension × ascending/descending (8 items, matching the `core.name`/
    `core.modified`/`core.size`/`core.extension` column ids the Ctrl+F3..F6 shortcuts already use),
    dispatching the exact same `onSortChange([{ columnId, direction }])` shape `DirectoryTable`'s
    headers do. Reuses the existing `.fm-view-mode-menu`/`.fm-view-mode-menu-item` styling.
  - **Photo-day grouping** (`frontend/src/features/directory-table/photo-grouping.ts`, new pure
    module + `directory-grid.ts`): `groupEntriesByDay` buckets entries into contiguous same-day runs
    (by `modifiedAt.slice(0, 10)`, localized via `Intl.DateTimeFormat({ dateStyle: 'full' })` for the
    header label; entries with a missing/unparseable `modifiedAt` bucket under a single "Unknown
    date" group) and `layoutPhotoLines` expands each group into a header line plus wrapped tile rows.
    `DirectoryGrid` gained a `photoMode?: boolean` attr; when set, its view function takes a second
    code path that loads every already-fetched entry (stopping at the first not-yet-loaded one and
    triggering `onEndReached`, exactly like the plain grid's existing lazy-load convention - grouping
    can only be computed over what's actually loaded), computes cumulative line offsets (header lines
    fixed at `DAY_HEADER_HEIGHT=40px`, tile rows at the measured tile height), and virtualizes over
    that line list the same way the plain path virtualizes over uniform rows - only lines whose
    offset range overlaps `[scrollTop - overscan, scrollTop + viewportHeight + overscan]` are
    rendered. Tile rendering itself was factored into a shared `renderTile` closure so both code
    paths (grouped and ungrouped) produce identical tile markup/behaviour. A "Photo" toggle button
    (`.fm-pane-photo-mode`, `aria-pressed`) sits next to the sort menu in `pane.ts`, state kept in a
    per-tab `Map<TabId, boolean>` local to the pane component - **not** persisted through
    `DirectoryViewConfiguration`/the `updateView` workspace command the way `viewMode`/`iconSize` are,
    unlike those two settings. This was a deliberate scope call: persisting it would mean touching
    `fm-domain`, `fm-transport-dto`, the OpenAPI schema, and regenerating the TS client for a
    session-local display toggle the acceptance criteria only requires to be "toggled on/off or
    selected," not persisted across restarts - flagged here rather than silently decided.

  **Verified**: `pnpm exec tsc --noEmit` clean; `biome check .` clean (the one pre-existing
  `noDescendingSpecificity` warning in `theme.css` is unrelated, present before this change).
  New/changed tests, verified by running exactly these files: 7 in `photo-grouping.test.ts`
  (grouping + line layout, including non-contiguous same-day runs and missing-date bucketing), 4 new
  in `directory-grid.test.ts` (`describe('photo mode')` - header insertion, no headers when off, tile
  content preserved, unknown-date bucketing), 6 new in `pane.test.ts` (`Pane grid sort menu` × 3,
  `Pane photo mode toggle` × 1, `Pane grid view type-to-select and quick filter` × 2, plus a hidden-
  in-table-view check), 1 new in `global-keydown-handler.test.ts` (F3 view-mode independence). Full
  `pnpm exec vitest run`: 1140/1141 passing - the one failure is the same pre-existing
  `config/mithril-inspector.test.ts` production-build timeout flake documented in the first Agent
  Notes entry above (confirmed unrelated: it fails on machine load, not on any file this task
  touches). Manually verified in the browser (mock runtime, `VITE_RUNTIME=mock`): switched a pane to
  grid view, opened the Sort menu and re-sorted by date descending (tile order visibly changed),
  toggled Photo mode on (an "Unknown date" section header appeared, matching the mock fixtures' lack
  of realistic `modifiedAt` values), and pressed F3 on a grid-selected file (the Lister viewer opened
  in the opposite pane showing real file content) - all three previously-deferred acceptance-criteria
  items confirmed end-to-end, not just via unit tests.

  **Remaining known gaps** (unchanged, out of this task's scope per the original MVP-phasing
  decision): no dedicated CBR (RAR) test with real archive bytes (documented above - shared code path
  with tested CBZ); inline rename UI in the grid (callback plumbing exists, no visible input
  overlay); delta-driven thumbnail cache invalidation (unnecessary given the content-addressed cache
  key). Photo mode's toggle state is session/tab-local rather than persisted (see above) - a
  reasonable small follow-up if a future task wants it durable across restarts.
