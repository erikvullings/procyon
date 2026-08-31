# 0068 Recursive filesystem search

Status: done
Priority: low
Owner: unassigned
Agent: unassigned
Area: backend
Depends on: 0067, 0032

## Context
`file-manager-coding-agent-spec.md` §24 item 2 and §37 (recursive search is part of version 1).

## Acceptance Criteria
- `fm-search` performs cancellable recursive traversal of one or more roots in Rust, streaming
  results as they are found rather than collecting the whole result set.
- Filename matching first (substring and glob); size/date/type filters and content search are
  designed for but explicitly deferred (§24).
- Results stream to the frontend over the event stream with backpressure/batching so a search
  matching 100,000 files does not flood the UI (§28).
- Traversal is cycle-protected, does not follow symlinks by default, and skips unreadable
  directories with a counted warning instead of aborting.
- A search is cancellable from the UI and cancels promptly mid-traversal.
- Results are exposed as a virtual location `search://local/{searchId}` so the existing pane and
  table render them unchanged (§24).
- Opening a result navigates to its containing directory with the entry selected.
- Integration tests: match counts on a fixture tree, cancellation, unreadable directory handling,
  symlink cycle, Unicode queries.

## Implementation Notes
- The search provider is the first non-local provider — it exercises the VFS abstraction (0016) and
  will reveal any leaked local-filesystem assumptions.
- Bound concurrency so search cannot starve navigation or operations.

## Agent Notes
- 2026-08-02 copilot: Finished and verified a substantial, high-quality implementation that had
  been left uncommitted by a prior session (matches the "subagent cut off mid-task" pattern seen
  on earlier tasks tonight) — read every new/changed file end-to-end before touching anything, then
  closed the remaining gaps rather than redoing the work.
  - **Design as landed**: new `fm-search` crate — `SearchEngine::start` (`crates/fm-search/src/
    engine.rs`) resolves every root to a native path synchronously (rejecting a bad root before
    spawning anything), registers a `CancellationToken` in a shared `SearchResultsStore`
    (`store.rs`, a `Mutex<HashMap<Uuid, SearchState>>`), then hands traversal to
    `tokio::task::spawn_blocking` so recursive walking never blocks the async runtime navigation/
    operations depend on. `SearchFileSystemProvider` (`provider.rs`) is a `FileSystemProvider`
    advertising only `ProviderCapabilities::LIST`, serving paged reads straight from the shared
    store (offset/limit continuation tokens) so the existing pane/table/paging code renders
    `search://` results with zero special-casing — proven end-to-end by
    `apps/fm-server/tests/search_routes.rs`, which starts a real search over a temp-dir fixture and
    polls the *generic* `/api/v1/directories/list` endpoint until `hasMore: false`.
  - **`search://local/{searchId}` location**: `fm-domain`'s `Location::parse` special-cases the
    `search` scheme (`crates/fm-domain/src/location.rs`) — validates shape only (`local` authority +
    one non-empty UUID segment) and deliberately does *not* route through `ParsedFileUri`, so
    `to_native_path`/`join`/`name`/`parent` all correctly error for a `search://` location (asserted
    in `crates/fm-domain/tests/location_contract.rs`). Critically, each *result entry*'s
    `EntrySummary.location` is the real `file://` location of the matched file
    (`build_entry_summary` in `engine.rs`, via `Location::from_native_path`), not a synthetic
    search-space address — this is what lets "open a result -> navigate to its containing directory
    with the entry selected" resolve for free via the entry's real location + `Location::parent()`,
    with no dedicated result-resolution endpoint needed. Confirmed present in the DTO/entry shape;
    no dedicated frontend UI (search bar, results view, click-to-navigate handler) exists yet to
    consume it — out of scope for this task (`Area: backend`), left as a natural follow-on.
  - **Streaming/batching (spec §28)**: genuinely incremental, not one giant batch at the end.
    `run_search` flushes whenever the buffer reaches `BATCH_SIZE = 500` matches *or* a partial
    buffer has been held for `BATCH_INTERVAL = 100ms`, whichever comes first, publishing each flush
    as a `BackendEventPayload::SearchResultsBatch { search_id, entries, is_complete, warnings_count }`
    event (new variant, `crates/fm-events/src/lib.rs`) scoped to the workspace via
    `EventAudience::Workspace`. A final flush (always sent, even if empty) carries
    `is_complete: true` so listeners can reliably detect the end. Verified directly with
    `streams_matches_across_nested_directories` (asserts real per-directory batches arrive) and
    `cancellation_stops_traversal_promptly`.
  - **Found and fixed one concrete gap**: the frontend's event-stream layer maintains hand-written
    allowlists of recognised event-type names (`KNOWN_EVENT_TYPES` in
    `frontend/src/api/events/event-stream.ts`, `EVENT_TYPES`/`HIGH_FREQUENCY_TYPES` in
    `sse-event-stream.ts`, `HIGH_FREQUENCY_TYPES` in `tauri-event-stream.ts`) plus a hand-written
    `BackendEventPayload` union mirror (`frontend/src/models/events.ts`) — none of these had been
    updated for `search.resultsBatch`, so despite the backend genuinely streaming batches, the SSE
    transport would never even register a named listener for them (browser/Axum host) and the
    shared `parseBackendEvent` allowlist would silently drop them as "unknown" everywhere else,
    fully defeating the streaming acceptance criterion for any real consumer. This was small and
    mechanical (register the type name in three allowlists + add the TS payload variant reusing the
    existing `EntrySummary` model, treating it as high-frequency alongside `operation.progress`/
    `directory.delta` since batches can arrive every 100ms) so fixed it directly rather than just
    flagging it; added a regression test (`recognises a search.resultsBatch event (task 0068)
    instead of dropping it as unknown` in `event-stream.test.ts`) so a future payload-shape rename
    can't silently regress this again. No UI yet consumes the now-deliverable events (see previous
    bullet) — that remains explicitly out of scope.
  - **Cycle protection / unreadable dirs / cancellation**: `walkdir::WalkDir::with(&root)
    .follow_links(false)` never descends into symlinked directories, which is both the "don't follow
    symlinks by default" requirement and sufficient cycle protection with no extra visited-inode
    bookkeeping (`symlink_cycles_do_not_cause_infinite_traversal`, 10s watchdog timeout, asserts
    completion). `walkdir` entry errors (permission-denied, race-deleted, etc.) increment a
    `u32` warning counter instead of aborting (`unreadable_directories_are_skipped_with_a_warning_
    not_an_abort`, a `#[cfg(unix)]` test chmod'ing a subdirectory to `0o000`). Cancellation is
    checked once per `WalkDir` entry (`cancellation.is_cancelled()`), and
    `cancellation_stops_traversal_promptly` proves a 2,000-file tree is interrupted well before
    completion rather than merely "eventually" stopping.
  - **Matching (`matcher.rs`)**: plain case-insensitive substring by default; automatically switches
    to a small hand-rolled `*`/`?` glob matcher (classic iterative two-pointer algorithm, operating
    on `Vec<char>` so multibyte UTF-8 is never split) when the query contains a wildcard. Unicode
    correctness verified for both modes (`matching_is_unicode_aware_not_ascii_only`,
    `unicode_queries_match_unicode_filenames` with real Japanese/emoji filenames). Size/date/type
    filters and content search are explicitly out of scope per spec §24 — confirmed nothing in the
    landed code attempts them; `StartSearchRequestDto`'s doc comment says so directly.
  - **Test inventory**: 18 unit/integration tests in `fm-search` (engine 7, matcher 4, provider 3,
    store 4) covering every acceptance-criterion scenario (fixture match counts, cancellation,
    unreadable directory, symlink cycle, Unicode query) plus 2 DTO round-trip tests in
    `fm-transport-dto`, 3 end-to-end REST tests in `apps/fm-server/tests/search_routes.rs` (happy
    path via the generic directory-list endpoint, cancel + not-found, empty-roots 400), and 1 new
    frontend regression test.
  - **OpenAPI/Orval regen**: ran `bash scripts/export-openapi.sh` (+131 lines to
    `frontend/openapi/openapi.json` for the two new operations/schemas) then
    `bash scripts/generate-api.sh` (Orval v8.23.0) — generated
    `frontend/src/api/generated/models/{startSearchRequestDto,startSearchResponseDto}.ts` and
    updated `file-manager-api.ts`/`models/index.ts`. Re-ran both scripts a second time afterwards
    and confirmed zero further diff (not stale).
  - **Full verification**: `cargo build --workspace --offline` clean; `cargo test -p fm-search`
    (18/18), full `cargo test --workspace --offline --no-fail-fast` all green except the two known
    pre-existing failures unrelated to this task
    (`fm-server::plugin_routes::list_plugins_starts_empty_and_unknown_enablement_is_not_found`,
    `fm-vfs-local::metadata_is_separate_and_capabilities_are_truthful`, both documented in earlier
    tasks' Agent Notes tonight); `cargo clippy --workspace --all-targets --offline -- -D warnings`
    and `cargo fmt --all --check` both clean. Frontend: `pnpm --dir frontend run typecheck` clean,
    `pnpm run lint:frontend` (biome) clean, full `pnpm --dir frontend test -- --run`: 46 files / 318
    tests pass (317 pre-existing + the 1 new event-type regression test).
  - `frontend/src/features/panes/pane.ts`'s pre-existing 2-line uncommitted diff
    (`input.fm-path-input` -> `input[type=text].fm-path-input`) was read and judged unrelated to
    search — it matches human commit `d8a1f68`'s own stated purpose ("command search input with
    type to overrule material-css", a Materialize-CSS styling fix for the path/command inputs, not
    filesystem search) — so it was left uncommitted, untouched, exactly as instructed.
    `frontend/src/api/client/tauri-file-manager-client.ts`'s pre-existing unrelated 1-line
    `import type` change was likewise left untouched, as with every prior task tonight. Human commit
    `d8a1f68` was not touched, amended, or rebased.
- 2026-08-05 codex: Replaced the filename-search modal's accumulating results with the existing
  `search://local/{searchId}` pane location. Search batches refresh the virtual provider through
  normal paged navigation; rows show decoded full paths and retain a leading `..` that uses tab
  history to return to the pre-search folder. Activating a result navigates to its real containing
  directory with the filename selected, while operations continue to use its real provider
  location. Extended the mock provider to exercise the generic listing contract. Added a separate
  dense Procyon override stylesheet for the mm modal/button/form/select/switch/dropdown components,
  loaded after mm's modular core/forms/components/utilities styles (unused picker and advanced
  groups are excluded). Verified frontend typecheck and lint clean;
  full frontend suite: 64 files, 559 passed and 1 skipped (three new behavior/contract tests).
- 2026-08-05 codex: Follow-up hardened the filename-search dialog: submitting now blurs the query
  before the controlled ModalPanel can apply `aria-hidden`, and the modal has a stable accessible
  id. Searches opened from a `search://` result location reuse that search's original real root.
  Tightened the Procyon modal chrome and spacing, retained MM's FlatButton footer actions with a
  borderless treatment, and removed the close button border. Browser verification confirmed focus
  is outside the dialog before it becomes hidden. Full frontend suite: 65 files, 568 passed and 1
  skipped; frontend typecheck and production build clean.
