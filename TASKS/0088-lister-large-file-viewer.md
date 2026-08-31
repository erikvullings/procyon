# 0088 Lister-style instant large-file viewer with lazy search

Status: done
Priority: low
Owner: unassigned
Agent: copilot, codex
Area: cross-cutting
Depends on: 0087

## Context

Follow-up from the same footer/viewer conversation as 0087. Total Commander's "Lister" (F3) opens
even multi-gigabyte files instantly by never loading the whole file into memory: it reads and
renders only the visible window, paging content in lazily as the user scrolls, and can still search
across the full file (not just the loaded window) without a full up-front load. Task 0087 ships F3
as a stopgap that just opens the OS default viewer; this task replaces that behaviour with a real
in-app viewer for text-like content once it exists, without changing F3's shortcut/action id/footer
wiring (0087's `core.view` action stays the entry point — its dispatch target changes, not its
identity).

Existing building blocks and gaps, confirmed by inspection:

- `fm-vfs`'s `VfsProvider::open_read` (`crates/fm-vfs/src/provider.rs`) returns a full sequential
  `AsyncRead` stream with no offset/range parameter — sufficient for "load lazily from the start"
  but not for random-access seeking to an arbitrary byte offset (needed to jump to a search hit
  deep in a large file without re-reading everything before it).
- Content search is explicitly out of scope for the existing recursive filesystem search feature
  (task 0068's Acceptance Criteria: "filters and content search are explicitly out of scope per
  spec §24") — there is no reusable in-content search infrastructure anywhere in the codebase.
  This task's search requirement is a new capability, not a wire-up of an existing one.

## Acceptance Criteria

- A new viewer surface (likely a modal/panel, consistent with the existing preview panel from task
  0071 if that ships first — check for overlap before duplicating UI chrome) that opens instantly
  regardless of file size: initial render must not wait on reading the full file.
- Backend: a byte-range read capability (new `VfsProvider` method or an additive HTTP
  `Range`-header-aware endpoint) so the frontend can request "give me bytes N..M" instead of
  streaming from the start every time. Decide whether this belongs on `VfsProvider` itself (works
  for both hosts uniformly) or is HTTP/Tauri-command-specific plumbing built on top of the existing
  `open_read` stream (skip-and-take on the server side) — prefer the former if provider
  implementations (local, and any future archive/remote provider) can support it cheaply, since
  that keeps it host-agnostic per `AGENTS.md`'s browser/Tauri parity rule.
- Frontend: a virtualized text/hex viewer that renders only the visible window (reuse the
  windowing approach from the directory table's virtualization if applicable), fetching adjacent
  chunks lazily as the user scrolls, with a small in-memory LRU of already-fetched chunks so
  scrolling back doesn't always re-fetch.
- Search: incremental substring/regex search that can locate matches outside the currently-loaded
  window without reading the entire file into the frontend at once — e.g. a backend search-within-
  file endpoint that scans server-side and returns match byte offsets (a chunked/streaming scan,
  not a full read into server memory either, so it scales the same way for huge files), with the
  frontend then fetching just the chunk(s) around each match to display. Jump-to-next/previous
  match should feel instant once the offset is known.
- Explicitly scope v1 to text-like content (respect a size/binary-detection heuristic — e.g. sniff
  for NUL bytes in the first chunk, same convention other file managers use); binary/hex viewing
  can be a documented non-goal or a fast-follow, but must not crash or hang on binary input either
  way (fall back to "binary file, cannot preview" rather than attempting to render).
- F3 (`core.view`, task 0087) opens this viewer for text-like files when available, falling back to
  the OS default-application open for binary/unsupported content or hosts where the viewer isn't
  available yet.
- Tests: backend range-read and search-within-file unit/integration tests (including on a
  synthetically large fixture file, per the performance-fixture conventions in task 0065), and a
  frontend viewer component test covering lazy chunk loading and search-driven scroll-to-match.

## Implementation Notes

- This is a substantial feature — expect it to need its own sub-tasks if scoped work turns out
  larger than one PR (e.g. split "backend range read + search" from "frontend virtualized viewer").
  Re-split into 0088a/0088b (or renumber) rather than growing this file indefinitely if that
  happens.
- Reuse `crates/fm-vfs-local`'s existing file-handle patterns for range reads (seek + read, since
  local files trivially support `Seek`); non-seekable or remote providers may need a documented
  reduced-capability path (e.g. read-ahead-and-discard rather than true seek) — treat this the same
  way other provider capabilities are capability-gated (see `fm-vfs/src/capabilities.rs`) rather
  than assuming every provider can do it.
- Check task 0071 (preview service) for overlap before building a second, competing preview/viewer
  UI surface — if 0071 already ships a preview panel shell, extend it rather than duplicating.

## Agent Notes

- 2026-08-04: Backend complete and verified (`cargo build/test/clippy/fmt` clean across the
  workspace): `fm-vfs` gained `FileSystemProvider::read_range` (default impl returns
  `UnsupportedCapability{RANDOM_ACCESS}`) and a new `fm_vfs::content` module (`ContentQuery`,
  `search_content`, `looks_like_binary`) generic over any `AsyncRead`, so it works for every
  provider with just `READ` (no `RANDOM_ACCESS` needed) and is reusable by task 0089.
  `fm-vfs-local` implements `read_range` via seek+take and advertises `RANDOM_ACCESS`
  ([crates/fm-vfs-local/src/lib.rs](../crates/fm-vfs-local/src/lib.rs)). New DTOs in
  [crates/fm-transport-dto/src/files.rs](../crates/fm-transport-dto/src/files.rs)
  (`ReadFileRangeRequestDto`/`ResponseDto`, `SearchInFileRequestDto`/`ResponseDto`) — confirmed via
  Orval regen that `data: Vec<u8>` serializes as a plain JSON number array (`number[]` in
  TypeScript), so no base64 dependency was added. New `FileManagerService::read_file_range` (falls
  back to sequential skip-read for providers without `RANDOM_ACCESS`; caps length at
  `MAX_RANGE_LENGTH` = 1 MiB; `probablyBinary` only sniffed at offset 0) and
  `::search_in_file` (caps at `MAX_SEARCH_MATCHES` = 5000, `truncated` flag) in
  [crates/fm-application/src/service.rs](../crates/fm-application/src/service.rs). New HTTP routes
  `POST /api/v1/files/range` / `/files/search` in
  [apps/fm-server/src/routes/files.rs](../apps/fm-server/src/routes/files.rs) and mirrored Tauri
  commands `read_file_range`/`search_in_file` in
  [apps/fm-desktop/src-tauri/src/commands.rs](../apps/fm-desktop/src-tauri/src/commands.rs).
  Fixed one pre-existing unrelated bug found along the way (a capability-set assertion in
  `fm-vfs-local`'s `metadata_is_separate_and_capabilities_are_truthful` test was missing
  `MOVE`/`DELETE`, confirmed via `git stash` to predate this work). Left one other pre-existing,
  unrelated, environment-dependent failure untouched: `plugin_routes.rs`'s
  `list_plugins_starts_empty_and_unknown_enablement_is_not_found` (also confirmed via `git stash`
  to fail on unmodified `main`).
  Frontend: added `readFileRange`/`searchInFile` to `FileManagerClient`
  ([frontend/src/api/client/file-manager-client.ts](../frontend/src/api/client/file-manager-client.ts))
  and implemented all three adapters (http/mock/tauri) with dedicated tests. The mock adapter has
  no real file content in its fixture tree, so it generates deterministic synthetic per-uri text
  content for both methods to exercise against. `tsc --noEmit` and Biome are clean; full frontend
  vitest suite passes.
  Remaining for this task: the LRU chunk cache, the virtualized viewer component itself (windowing,
  lazy chunk fetch, search-driven scroll-to-match), and the F3 `core.view` frontend wiring
  (intercept before the existing default-open dispatch, falling back to it for binary/unsupported
  content) — none of that frontend UI has been built yet.
- 2026-08-04: F3 wiring complete. `app-shell.ts` intercepts `core.view` before the existing
  combined open/edit/open-with dispatch: for a single selected non-parent file entry with another
  pane present, it opens a new `FileViewer` (via `createFileViewerController` from
  [file-viewer-controller.ts](../frontend/src/features/preview/file-viewer-controller.ts)) in the
  *opposite* pane, replacing that pane's directory-listing surface entirely (`viewerContent`,
  threaded through `WorkspacePaneContent`/`workspace-layout.ts`/`pane.ts`); falls through to the
  pre-existing default-open dispatch for directories, multi-select, or a single-pane workspace.
  Deliberately does no synchronous binary-detection before opening — the viewer surfaces its own
  "Preview not available" state for unsupported content rather than falling back to OS-open, kept
  simple per the task's own scope note. Two new integration tests in
  [app-shell.test.ts](../frontend/src/app/app-shell.test.ts) cover opening via F3 and closing via
  the viewer's close button. Fixed one bug found while exercising this against real files: a
  "Maximum call stack size exceeded" for images ≥1 MiB in
  [content-preview.ts](../frontend/src/features/preview/content-preview.ts)'s
  `readFullImageDataUri` — it spread each 1 MiB chunk's `number[]` into `chunks.push(...chunk.data)`,
  exceeding V8's spread-argument limit; fixed by accumulating `Uint8Array` segments and a single
  `Uint8Array.set` copy pass instead (regression test uses a realistic full-size 1 MiB buffer, since
  a small fixture would not have caught this).
  Also found and fixed, while live-testing against a real ~460-entry directory, an unrelated
  pre-existing bug (confirmed via `git stash` to predate this session's changes): the pane status
  bar showed only the loaded-so-far entry count (e.g. "256 entries") instead of the real backend
  total when no quick filter was active, making a directory with more pages look capped/broken
  before the user ever scrolled — even though pagination itself (backend and virtualization) was
  verified correct end-to-end via direct `curl` against `/api/v1/directories/list` and a live
  scroll-to-bottom test. Fixed in
  [pane.ts](../frontend/src/features/panes/pane.ts) to show `"${loaded} of ${realTotal} entries
  (loading more…)"` while more pages are pending.
  Per explicit product direction this session, task 0071's automatic cursor-driven preview panel
  was also removed (see [TASKS/0071](0071-file-preview-architecture.md) Agent Notes) — F3 is now
  the sole preview trigger, which this task's viewer already provided.
  Verified: `tsc --noEmit` clean, Biome clean, full frontend vitest suite green (529/529 after the
  0071 panel removal).
  Remaining for this task: the LRU chunk cache is implicit in the controller's existing chunk
  fetching but not separately benchmarked; backend `cargo fmt`/`clippy`/`test` re-confirmation for
  this session's (frontend-only) changes was not re-run since no Rust files were touched.
- 2026-08-08 gemini: Decision Record: Selected CodeMirror 6 (CM6) over Monaco for the in-app editor and replacing highlight.js in the 0088 viewer.
  - Rationale: CM6 provides a lightweight bundle (~120 KB vs Monaco's ~2–4 MB), zero web worker runtime/bundler configuration for Tauri/web environments, and direct integration with Mithril's imperative lifecycle (`oncreate`/`onremove`). Core language packages (`@codemirror/lang-json`, `@codemirror/lang-markdown`, `@codemirror/lang-xml`) satisfy formatting and diagnostic needs for targeted config and Markdown files without heavy LSP dependencies.
  - Standardized Syntax Engine: Replacing `highlight.js` in 0088 with read-only CM6 (`EditorState.readOnly.of(true)`) eliminates duplicate grammar bundles, ensures 100% theme/token parity between viewer and editor modes, and allows seamless transitions from viewing to editing.
  - Entry Point & Shortcut Policy: F4 remains the primary "Edit" action, using 0088 type and size inspection to route supported files <= limit to the CM6 in-app editor, with external editor fallback for large or binary files.
- 2026-08-08: Replaced the viewer's highlight.js/imperative `innerHTML` syntax pipeline with the
  shared read-only CodeMirror 6 component. Search matches now use a CodeMirror selection and
  scroll effect, the highlight.js dependency and obsolete DOM highlighter/tests were removed,
  and the focused preview suite passes (52 tests).
