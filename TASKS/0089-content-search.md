# 0089 Content search across files

Status: done
Priority: low
Owner: unassigned
Agent: unassigned
Area: cross-cutting
Depends on: 0068

## Context
Total Commander's Alt+F7 "Find files" dialog searches both filenames and file *contents* (grep-like,
with regex support) in one place. Task 0068 shipped the filename/glob half of this as the
`fm-search` crate + `search://local/{searchId}` virtual location, but its Acceptance Criteria
explicitly deferred content search ("filters and content search are designed for but explicitly
deferred (§24)") and its Agent Notes confirm content search was NOT attempted in the landed code.

More importantly: **there is no frontend UI to trigger or view a search at all yet**, for either
filenames or content — 0068 shipped the backend engine and OpenAPI-generated `startSearch` client
function only; 0068's own Agent Notes call this out directly ("no dedicated frontend UI (search
bar, results view, click-to-navigate handler) exists yet to consume it ... left as a natural
follow-on"). Confirmed by inspection: nothing in `frontend/src` references `startSearch` outside
the generated API client. This task therefore necessarily includes standing up that missing search
entry point, not just adding a content-matching mode to an existing dialog.

## Acceptance Criteria
- A search dialog/panel (entry point: a new `core.findFiles` action, Total-Commander-style default
  shortcut `Alt+F7`) with: a filename/glob query (reusing 0068's existing filename search
  unchanged), an optional content-search query (plain substring by default, opt-in regex — mirror
  the "regex opt-in and validated before use" convention already used for 0072's multi-rename), and
  a scope (current directory / current directory + subdirectories / one or more chosen roots).
- Backend: extend `fm-search`'s traversal to optionally scan matched (or all, if no filename filter)
  files' contents for the content query, without reading an entire huge file into memory at once —
  chunked/streaming scan per file, bounded per-file time/size so one huge file cannot stall the
  whole search (reuse the streaming-scan mindset from task 0088's Lister search, but do not block on
  0088 landing first — these are independent features that happen to share a scanning approach).
- Skip binary files by heuristic (same NUL-byte sniff convention noted in task 0088) rather than
  attempting a text match against binary content.
- Results stream to the frontend the same way 0068's filename results do (batched
  `search.resultsBatch` events over the existing event stream) — content matches carry enough
  information to jump to the first (or each) match's line/offset in the file, not just "this file
  matched".
- Frontend results view: a virtualized list (reuse the directory-table windowing approach) showing
  matched files with total match count; activating a result navigates to its containing directory
  with the entry selected (0068's existing per-entry `location` already supports this for the
  filename-only case — verify/extend for the content-match case too).
- Search is cancellable and cancels promptly mid-traversal (same `CancellationToken` pattern as
  0068).
- Tests: `fm-search` unit/integration tests for content matching (including a binary-file fixture
  that must be skipped, and a large-file fixture to confirm bounded scanning), Vitest tests for the
  new dialog/results-view component, and an end-to-end route test mirroring
  `apps/fm-server/tests/search_routes.rs`.

## Implementation Notes
- `crates/fm-search/src/engine.rs` (`SearchEngine::start`), `matcher.rs`, `provider.rs`, and
  `store.rs` are the existing pieces to extend — read task 0068's Agent Notes in full before
  starting, they document the current design precisely (batching thresholds, cancellation
  checkpoints, `search://` location handling).
- Do not build a second, competing search entry point if a "quick filter" or similar per-pane UI
  already partially overlaps — task 0067 (quick filter) is explicitly local/client-side and
  distinct from this (see `TASKS/0067-quick-filter.md`: "Distinct from filesystem search (0068) —
  this never hits the backend"), so no overlap there, but double-check no other in-flight task has
  since added a search UI before starting.
- OpenAPI/Orval regen required if `StartSearchRequestDto`/`StartSearchResponseDto` gain new fields
  (content query, scope) — see `AGENTS.md` "Generated code".

## Agent Notes
- 2026-08-03: Implemented the **filename-search slice only** (content/grep scanning explicitly
  NOT attempted — status intentionally left `open`). Added `core.findFiles` (`Alt+F7`, Total
  Commander convention, no selection requirement) to `crates/fm-application/src/action.rs`, wired
  through the mock action fixture (`fixtures/mock-responses/actions.json`).
- Added `startSearch`/`cancelSearch` to the semantic `FileManagerClient` interface
  (`frontend/src/api/client/file-manager-client.ts`) and implemented them in all three adapters
  (HTTP, mock, Tauri) so UI code never calls the raw generated Orval `startSearch`/`cancelSearch`
  functions directly. Tauri adapter forwards to new `start_search`/`cancel_search` commands in
  `apps/fm-desktop/src-tauri/src/commands.rs` (registered in `lib.rs`), which call the same
  `FileManagerService` methods as the REST route from task 0068.
- New dialog: `frontend/src/features/search/find-files-dialog.ts`, following the `ModalPanel`
  focus/blur pattern from `create-directory-dialog.ts`. Replaced the dead `m('span', 'Search')`
  placeholder in `frontend/src/app/app-shell.ts` with a real "Find files" entry point that opens
  the dialog, starts a search via the client, and reuses 0068's existing `search://local/{searchId}`
  results machinery (via `search.resultsBatch` events) rather than a bespoke list — activating a
  result navigates the active pane to its containing directory with the entry selected
  (`navigation.ts`'s `navigate()` gained an optional `preferredCursorName` parameter for this).
- Content-search query field, regex opt-in, per-file streaming/bounded scanning, binary-file
  skip heuristic, virtualized results view, and the `fm-search`/`fm-server` backend content-matching
  work described in the Acceptance Criteria above are all still outstanding — this task remains
  open until that half lands.
- 2026-08-07 copilot: Completed the content-search half. New `scanner.rs` in `fm-search` provides
  bounded per-file scanning (10 MiB max, 200 ms timeout, NUL-byte binary sniff, cancellation)
  reusing `fm_vfs::content::search_content`. `SearchEngine::start` now accepts `SearchOptions`
  with optional `ContentQuery` and `recurse` flag. `EntrySummaryPayload` gained `contentMatches:
  Option<Vec<ContentMatchSummary>>` for line/offset match info in the event stream.
  `StartSearchRequestDto` gained `contentQuery`, `contentRegex`, `contentCaseSensitive`,
  `contentWholeWord`, `recurse` fields (all optional with sensible defaults for back-compat).
  Frontend: extended `FindFilesDialog` with content query field, regex toggle, and recurse toggle.
  All new fields flow through the semantic `FileManagerClient` to the generated API.
  OpenAPI/Orval regenerated. `ContentMatchSummary` type added to `fm-events` and frontend
  `EntrySummary` model.
  **Verified**: `cargo test -p fm-search` 35/35 (10 new scanner tests + 7 new engine content tests),
  `cargo test -p fm-transport-dto` 58/58 (2 new), E2E `search_routes.rs` 5/5 (2 new), frontend
  vitest 656 passed (3 new in dialog tests, 1 net pre-existing failure unchanged), `cargo clippy
  --all-targets -- -D warnings` clean on affected crates, `cargo fmt --all --check` clean,
  `tsc --noEmit` clean (1 pre-existing error on unrelated line 1188/1175).
  **Known limitation**: `contentMatches` are only available in the `search.resultsBatch` event
  stream; the directory-listing endpoint (`/directories/list`) serves `EntrySummaryDto` without
  match metadata (the VFS provider path has no mechanism for per-entry annotations). If the frontend
  needs match counts/context without the event stream, this would require extending the VFS or
  adding a dedicated annotations API.
