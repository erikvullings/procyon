# 0119 Decompose FileManagerService into capability sub-services

Status: done
Priority: medium
Subsystem: backend
Depends on: none

## Context

The ` FileManagerService` facade (`crates/fm-application/src/service.rs`) has grown to ~5,800 lines, combining operation executor construction, plugin management, settings CRUD, search coordination, connection DTO conversion, file editing, archive operations, action invocation, workspace lifecycle, and icon serving. The interface is the entire struct (~40+ public methods), making the module shallow: understanding any one capability requires navigating the whole monolith.

This is the central architectural friction point in the Rust codebase. See `/improve-codebase-architecture` skill findings for the full analysis.

## Acceptance Criteria
- `FileManagerService` reduced to a thin composition layer. Original target was <500 lines; revised
  2026-08-14 per product guidance — ~1000 lines is an acceptable outcome, and extractions must be
  along genuine capability/responsibility boundaries. Do not force a split (e.g. arbitrarily
  breaking up `impl FileManagerService`'s method bodies into loops/helpers) purely to hit a line
  count; a clean stop above 500 lines is preferable to an artificial one at 500.
- Each capability cluster (operations, file editing, plugins, connections, search) extracted into its own deep module with a small, well-defined interface
- All extracted modules have their own test coverage — not just unit tests, but tests through the module's interface
- No behavioural changes visible to callers (Axum routes, Tauri commands, CLI)
- All existing integration tests pass
- Zero compiler warnings after the refactor

## Implementation Notes
- Strategic modules emerge naturally: `OperationOrchestrator`, `FileEditorService`, `ConnectionFacade`, `PluginManager`, and settings delegation
- The facade shrinks to field declarations + delegation calls
- Follow the pattern already established by `ConnectionService` (deep, ~970 lines, well-tested, ~20 tests) and `DirectoryService` (deep, ~1072 lines, well-tested)
- This task is the parent; subtasks 0120-0125 each extract one capability. This task coordinates and ensures the composition layer is correct after all subtasks land.

## Agent Notes

- 2026-08-14: Picked this up after confirming (via `wc -l`) the facade was still ~3,836 lines
  despite subtasks 0120–0125 all being done — the composition layer itself was never actually
  thinned. First extraction pass in this session:
  - New module `crates/fm-application/src/operation_history.rs` (302 lines): moved
    `OperationHistory` (crash-safe operation-snapshot persistence beside settings, prune/save,
    restart-recovery marking in-flight operations as `Interrupted`), `ApplicationOperationObserver`
    (bridges the scheduler's `OperationSnapshotObserver` callback to both history persistence and
    refreshing affected directory listings), and the pure `operation_dto`/`operation_result_summary`
    conversion functions they depend on. This cluster was fully self-contained (only real dependency
    was `DirectoryService`, already public from the crate root) and had exactly one dedicated unit
    test (`restarted_service_restores_inflight_history_as_interrupted`, left in `service.rs`'s test
    module since it exercises `FileManagerService::new` end-to-end, not `OperationHistory` in
    isolation — appropriate as a facade-level integration test).
  - `service.rs`: 3,836 → 3,555 lines. Added `ApplicationOperationObserver::new(...)` constructor
    (the old code built it via a public-field struct literal, which no longer works once the fields
    are private to the new module). Fixed all resulting import fallout (`OperationSnapshotObserver`,
    `HashSet`, `std::io::Write` no longer needed at the top level; `OperationStateDto` moved into the
    test module's own `use`, since every usage was test-only and caused an unused-import warning in
    non-test builds otherwise).
  - Verified: `cargo build -p fm-application --tests` zero warnings, `cargo clippy -p fm-application
    --all-targets -- -D warnings` clean, `cargo fmt -p fm-application` clean, `cargo test -p
    fm-application --lib` 183/183 passing (including the moved-behavior test), `cargo test -p
    fm-application` (all integration test binaries) all green, `cargo build --workspace` clean.
  - **Not done**: the facade is still ~3,555 lines against the <500-line target — this single pass
    is real but modest progress (~7% reduction), not the full decomposition. Left `Status:
    in_progress` rather than `done`; do not mark this task done until the line-count target is
    actually met.
  - **What's left in `service.rs`, roughly in extraction-priority order**:
    1. A cluster of ~15 free-standing pure mapping/conversion functions (~400 lines, currently
       lines ~1460–1860): `map_scheduler_error`, `copy_request`, `delete_request`,
       `comparison_entry_side_dto`, `comparison_entry_dto`, `sync_plan_item_dto`, `operation_kind`,
       `mutating_operation_kind`, `platform_action_kind`, `map_platform_error`,
       `map_file_icon_error`, `settings_to_dto`, `settings_from_dto`, `detect_platform`. Same shape
       as the just-extracted `operation_dto` — no `self` dependency, straightforward to move to a
       `service_mappings.rs` (or split further: settings mapping vs. comparison mapping vs. platform
       mapping are three unrelated concerns bundled only by "free function in this file").
    2. The `impl FileManagerService` block itself (~1,330 lines) is where the real facade-shrinking
       work is: per the Implementation Notes below, capability clusters like search/comparison
       coordination (`start_search`/`cancel_search`/`start_comparison`/`generate_sync_plan`/
       `apply_sync_plan`, currently thin-ish wrappers already delegating to `fm-search`/
       `fm-comparison` engines — check whether they're thin enough to leave as delegation or need
       a coordinating module of their own) and action invocation (`invoke_action`, ~125 lines) are
       the biggest remaining named clusters that don't yet have a dedicated module the way
       operations/file-editing/connections/plugins already do.
    3. **The `#[cfg(test)]` block is ~1,690 of the file's ~3,555 lines** (from `mod tests` at
       ~line 1863 to EOF) — this is the elephant in the room for hitting <500 lines. Reaching the
       target requires moving each extracted capability's tests into that capability's own module
       alongside the code (as this pass did for the one `OperationHistory`-specific test), not just
       shrinking the non-test code. Facade-level integration tests (constructing a real
       `FileManagerService` end-to-end) belong in `service.rs`; anything testing one capability in
       isolation should move with that capability.
  - Stopped here for this session rather than chaining further extractions without re-verifying
    each one — each subsequent pass should follow the same pattern (extract self-contained cluster
    → new module → fix imports → `cargo build --tests` zero warnings → `clippy -D warnings` → `fmt`
    → `cargo test -p fm-application --lib` and the full integration suite → `cargo build --workspace`)
    before moving to the next cluster, rather than batching multiple extractions unverified.
- 2026-08-14 (second pass, same session): product guidance revised the target to ~1000 lines and
  explicitly asked for clean capability boundaries over forced splitting — see the "Not done" item
  above and the revised Acceptance Criteria. Extracted the ~400-line mapping-function cluster
  identified in the first pass's notes, split by actual responsibility rather than dumped into one
  file, since "free function defined in service.rs" was never a real boundary:
  - `crates/fm-application/src/settings_mapping.rs` (134 lines): `settings_to_dto`/
    `settings_from_dto` — `Settings` (`fm-settings`) <-> `SettingsDto` (`fm-transport-dto`).
  - `crates/fm-application/src/comparison_mapping.rs` (98 lines): the full `fm-comparison` <->
    transport-DTO conversion set for directory comparison/sync (task 0075) — `comparison_criteria`,
    `comparison_criteria_dto`, `comparison_status_dto`, `comparison_entry_side_dto`,
    `comparison_entry_dto`, `sync_mode`, `sync_action`, `sync_action_dto`, `sync_plan_item_dto`.
  - `crates/fm-application/src/operation_requests.rs` (141 lines): translates wire-level requests/
    action ids into the operations engine's own types — `map_scheduler_error`, `copy_request`/
    `delete_request` (sync-plan-row request builders), `operation_kind`, `conflict_policy`,
    `mutating_operation_kind`.
  - `crates/fm-application/src/platform_mapping.rs` (74 lines): action-id-to-platform-adapter
    dispatch mapping (task 0061) plus OS detection — `PlatformActionKind`, `platform_action_kind`,
    `map_platform_error`, `map_file_icon_error`, `detect_platform`.
  - All four are pure functions/enums with no `self`/facade-state dependency, same shape as
    `operation_history.rs`'s `operation_dto`. Every external call site stayed inside
    `impl FileManagerService` (confirmed by grep before extracting, not assumed) — only
    `pub(crate)`-exposed what's actually called from `service.rs`; helpers used solely within a new
    module's own functions (`comparison_entry_side_dto`, `sync_action_dto`, `comparison_status_dto`)
    stayed module-private.
  - `service.rs`: 3,555 → 3,160 lines (395 lines moved). Fixed import fallout the same way as the
    first pass, including one `cargo fix --lib -p fm-application --tests --allow-dirty` quirk worth
    flagging for next time: it over-removed `OperationKindDto`/`PlatformKindDto` from the crate-wide
    import list because they're only used inside `#[cfg(test)] mod tests`, then failed the
    `--tests` build it had just "fixed" — `cargo fix` doesn't reliably reconcile a single import
    list against both the plain and `--tests` compilations in one pass. Manually moved both into the
    test module's own `use fm_transport_dto::{...}` (matching the `OperationStateDto` pattern from
    the first pass) rather than trusting the tool's second pass.
  - Verified identically to the first pass: `cargo build -p fm-application --tests` zero warnings,
    `cargo clippy -p fm-application --all-targets -- -D warnings` clean, `cargo fmt -p
    fm-application` clean, `cargo test -p fm-application --lib` 183/183 (same count — no coverage
    lost), `cargo test -p fm-application` (full integration suite) green, `cargo build --workspace`
    clean.
  - Session total so far: service.rs 3,836 → 3,160 lines (~18% reduction) across two verified
    passes, five new modules (`operation_history`, `settings_mapping`, `comparison_mapping`,
    `operation_requests`, `platform_mapping`).
  - **What's left** is unchanged in kind from the first pass's item 2 and 3 (see above): the
    `impl FileManagerService` block itself (~1,330 lines, 61 public methods) is the real remaining
    facade-shrinking work, and it's architecturally different from what's been extracted so far —
    these methods are genuinely tied to `&self` and the facade's many fields, so extracting them
    means designing real sub-service types (following the `ConnectionFacade`/`PluginManager`
    pattern) that own the relevant state, not just moving pure functions. That's a bigger design
    task than this session's mechanical extractions and is better done as its own focused pass. The
    ~1,690-line test module is the other big piece — same "move tests alongside their capability"
    approach as before, but most of what's left to extract now needs the sub-service design decided
    first, since tests for `&self` methods can't move to a module that doesn't exist yet.
- 2026-08-14 (third pass, same session): requested final scan for anything still easy/safe before
  starting the harder sub-service redesign. Read every remaining `impl FileManagerService` method
  and classified each by real dependency count, rather than assuming — found five more genuinely
  single-field, low-risk methods the first two passes missed (they weren't free functions, so
  didn't show up in the "free function" sweep, but they don't touch `&self` state beyond one field
  either, same risk profile):
  - `system_locations` (only used `self.platform`) → moved into `platform_mapping.rs` as
    `discover_system_locations(platform: Arc<dyn PlatformAdapter>)`.
  - `runtime_capabilities` (only used `self.runtime` + `self.platform.capabilities()`) → moved as
    `runtime_capabilities_dto(runtime, capabilities)`, taking values instead of `&self`.
  - `read_file_range` and `search_in_file` (only used `self.providers`, ~120 lines combined,
    including their `skip_bytes`/`MAX_RANGE_LENGTH`/`MAX_SEARCH_MATCHES` helpers) → new module
    `crates/fm-application/src/content_streaming.rs` (166 lines), functions taking
    `&ProviderRegistry` instead of `&self`.
  - `enrich_snapshot`/`volume_capacity` (only used `self.platform`, ~30 lines) → `volume_capacity`
    moved into `platform_mapping.rs`; `enrich_snapshot` itself stayed (2 lines, calls the new
    function) since it's the one place that actually needs `self.directories`' snapshot type too.
  - Confirmed everything else already delegating one-to-three lines to `self.workspaces`/
    `self.connections`/`self.plugin_manager`/`self.remote_terminals`/`self.editor`/
    `self.directories`/`self.settings`/`self.events`/`self.archive_provider` is already exactly
    the "thin composition layer" the acceptance criteria wants — spot-checked every one (not
    assumed from the method name) and found no hidden logic worth extracting further there.
  - `service.rs`: 3,160 → 2,957 lines this pass. **Session total: 3,836 → 2,957 lines (~22.9%) across
    three verified passes and six new modules** (`operation_history`, `settings_mapping`,
    `comparison_mapping`, `operation_requests`, `platform_mapping`, `content_streaming`).
  - Verified identically to the prior two passes: zero warnings, clippy `-D warnings` clean, fmt
    clean, `cargo test -p fm-application --lib` 183/183 (same count throughout all three passes —
    no coverage lost across the whole session), full integration suite green, workspace build
    clean.
  - **Conclusion of this "easy/safe" sweep: there is nothing left in `service.rs` of meaningful size
    that is a low-risk extraction.** Everything remaining beyond thin delegations falls into four
    named clusters, all requiring real sub-service design (not mechanical moves) because each is
    genuinely tied to multiple fields and/or calls back into other facade methods:
    1. **Constructors** (`new`, `with_event_bus`, `with_platform_adapter`,
       `with_platform_adapter_and_credential_store`, ~230 lines) — wires every field on the struct;
       this is inherently facade-shaped work, not a candidate for extraction at all (it's the
       composition root the whole task is trying to shrink *around*, not a capability to pull out).
    2. **Operations management** (`start_operation`, `list_operations`, `list_operation_page`,
       `get_operation`, `cancel_operation`, `pause_operation`/`resume_operation`,
       `resolve_operation_conflict`, `force_cross_volume_moves_for_tests`, ~165 lines) — touches
       `operations` (Scheduler), `operation_idempotency`, `operation_history`, `planner`, and
       `force_cross_volume_moves`, plus `cancel_operation` falls back to `search`/`comparison` for
       ids the scheduler doesn't recognize. A genuine `OperationsCoordinator` sub-service candidate.
    3. **Search/comparison coordination** (`start_search`, `cancel_search`, `start_comparison`,
       `cancel_comparison`, `get_comparison_page`, `generate_sync_plan`, `apply_sync_plan`,
       ~230–250 lines) — touches `search`, `comparison`, `comparison_store`, `events`, and
       `apply_sync_plan` calls back into `self.start_operation`, so it can't be fully independent
       of whatever holds operations submission.
    4. **Action invocation** (`invoke_action`, `invoke_platform_action`, ~130 lines) — touches
       `plugin_manager`, `actions`, `platform`, `settings`, and calls back into
       `self.start_operation` for mutating actions. The most central, most coupled cluster.
  - These four total ~750–775 lines of real logic, plus the ~230-line constructor block that
    structurally can't shrink, plus the ~1,690-line test module (which can only shrink once the
    sub-services above exist for their tests to move alongside). That's the real shape of what's
    left: getting from ~2,957 to the ~1000-line target needs 2 (2 and 3 could plausibly merge, since
    3's cancel/apply paths already reach into 2) or 3 new sub-service types designed with the same
    care as `ConnectionFacade`/`PluginManager` — genuine architecture work for a dedicated session,
    not something to rush through opportunistically.
- 2026-08-14: Paused here by explicit product decision — the easy/safe extraction work (three
  passes above) is done and verified; the remaining sub-service design (constructors excluded —
  see the "Conclusion" above for why those can't shrink further) is deliberately deferred to a
  future dedicated session rather than continued now. `Status` set back to `open` (not
  `in_progress`, since nothing is actively being worked on) and `Priority` lowered to `medium`.
  Next agent picking this up: start by reading the third pass's "Conclusion of this sweep" note
  above — it already scopes the remaining work into four clusters with size estimates and the
  specific reason each one needs real design rather than mechanical extraction. No new
  investigation should be needed before starting; the open question is *how* to shape
  `OperationsCoordinator` / `SearchComparisonCoordinator` / `ActionInvoker` (naming and exact
  boundaries still undecided — the note above suggests 2 and 3 could plausibly merge into one
  type since `apply_sync_plan` already reaches into operations submission, but that's a design
  call for whoever picks this up, not settled here), not whether there's more low-hanging fruit
  first (there isn't).
- 2026-08-25: Ran `/improve-codebase-architecture` across the rest of the codebase (this task was
  excluded from that pass's scope since it's already fully scoped above). Four more deepening
  candidates found and tracked as their own tasks, none of which depend on or block this one:
  0152 (`Scheduler::run_job` in `fm-operations`, zero-tested concurrency logic with scattered
  cancellation checks), 0153 (`app-shell.ts` regrew from ~1,816 to 3,264 lines since the frontend
  deepening work in the "done" list below was marked complete — see `TASKS/README.md`, which now
  needs a correction), 0154 (132-branch if/else dispatch in `global-keydown-handler.ts`, low
  priority), 0155 (S3 provider multipart upload paths lack unit coverage, weakest/lowest-confidence
  finding). See each task file for full detail.
- 2026-08-25: Before resuming, re-checked `service.rs` against the "2,957 lines" figure from the
  third pass above — it has **regrown to 3,830 lines / 85 public methods** (`git log` shows the
  growth came from feature commits landing directly on the facade after the last verified pass:
  thumbnails/grid view, checksums, native menu bar content, S3 provider, WebDAV provider, and the
  multi-window workspace redesign, among others — not from any reversion of the prior extractions,
  which are still intact as their own modules). Same regrowth pattern as `app-shell.ts` (see 0153).
  Whoever resumes this task should re-run the "easy/safe" free-function/single-field sweep from the
  first three passes against the *current* file before assuming the four-cluster breakdown above is
  still exhaustive — new code added since may include more mechanically-extractable material, or
  may have grown one of the four named clusters (Operations management / Search-comparison
  coordination / Action invocation) further. Re-verify line/method counts per cluster before
  designing the sub-service split.
- 2026-08-26: Fourth extraction pass (subagent-initiated, main session took over verification and
  commit after the subagent's own background-build-wait loop failed to resume it three times in a
  row -- see process note at the end of this entry). Re-ran the easy/safe sweep against the regrown
  file per the note above and found new mechanically-extractable material that had accumulated
  since the third pass, confirming the note's suspicion:
  - New module `crates/fm-application/src/checksum_coordinator.rs` (585 lines): `ChecksumCoordinator`
    -- checksum computation (`start_checksums`/`cancel_checksums`/`get_checksum_page`/
    `render_checksum_file`), duplicate-file scanning (`start_duplicate_scan`/`cancel_duplicate_scan`/
    `get_duplicate_page`), and `save_checksum_file`. This cluster came from task 0077 (checksums and
    duplicate detection) and had grown as its own self-contained capability with no dedicated module,
    the same shape as `ConnectionFacade`/`PluginManager`.
  - `crates/fm-application/src/platform_mapping.rs` grew by ~180 lines: moved the remaining
    platform-native free functions out of `service.rs` -- `native_path_for`, `read_file_icon`,
    `read_finder_tags`/`write_finder_tags`, `read_spotlight_comment`/`write_spotlight_comment`, and
    `install_native_menu` (task 0133, native menu bar). Same "pure function, no facade-state
    dependency" shape as the third pass's finds.
  - `service.rs`: 3,830 -> 3,495 lines (~9% this pass).
  - **Two bugs found and fixed during verification** (both introduced by the extraction, not
    pre-existing): (1) `cancel_operation`'s conflict-fallback chain at two call sites still referenced
    `self.checksum` after the field was renamed to `self.checksums` -- caught by `cargo build`, not
    caught by the extraction itself since it's inside a `.or_else` chain type-inferred loosely enough
    to compile-error only late. (2) Two moved tests
    (`start_checksums_publishes_an_operation_created_event_and_returns_a_job_id`,
    `start_duplicate_scan_returns_a_scan_id_whose_page_is_immediately_queryable`) panicked with
    "there is no reactor running" because `ChecksumCoordinator`'s engine calls need a Tokio runtime;
    fixed by changing both from `#[test]` to `#[tokio::test]` + `async fn`, matching the existing
    pattern in `file_editor.rs` for the same reason. Neither bug was caught before a full `cargo build
    -p fm-application --tests` + `cargo test -p fm-application --lib` run -- a reminder that "moved a
    self-contained cluster" still needs the full verification chain, not just visual inspection.
  - Verified: `cargo build -p fm-application --tests` zero warnings, `cargo clippy -p fm-application
    --all-targets -- -D warnings` clean, `cargo fmt -p fm-application` clean, `cargo test -p
    fm-application --lib` 250/250 passing (248 + the 2 fixed tests), `cargo test -p fm-application`
    (full integration suite) green, `cargo build --workspace` clean. Committed as
    `9ccf1e6` (one commit for both extractions together, rather than the usual one-per-cluster
    granularity -- see process note below for why).
  - **Process note for future sessions**: the subagent doing this extraction correctly did the work,
    but three times in a row ended its own turn saying "waiting for a background build/Monitor
    notification" and was never actually resumed by that notification -- background bash jobs
    launched *inside* a subagent do not appear to wake that subagent the way they wake a top-level
    session. The main session had to take over waiting on the build process directly (via `ps`/PID
    polling) and finish verification/commit itself. If a future subagent session needs to run a
    build that could exceed a single turn, it should run it in the **foreground** with a long timeout
    rather than backgrounding it and expecting to be woken up -- or expect the coordinating session to
    take over verification, as happened here. This is also why this pass is a single combined commit
    instead of two separate ones: splitting the already-completed, already-verified diff back apart
    would have required re-running the ~55+ minute full verification chain a second time for a purely
    cosmetic granularity improvement, which wasn't worth the cost here.
  - **What's left**: the four-cluster breakdown from the third pass (Operations management,
    Search/comparison coordination, Action invocation, plus the un-shrinkable constructors) is
    unchanged in kind, but sizes should be re-verified again before resuming -- this pass's sweep
    only covered checksums/platform-native material, not those four clusters, and code has continued
    to land on `service.rs` since. Next agent: re-run `wc -l`/method-count checks on the four named
    clusters before assuming the third pass's ~750-775 line estimate still holds, then proceed with
    the `OperationsCoordinator`/`SearchComparisonCoordinator`/`ActionInvoker` design work per the
    third pass's conclusion.
- 2026-08-27 Copilot: Completed the remaining capability design. Added
  `OperationsCoordinator` (scheduler submission, idempotency, history projection, paging, pause/
  resume/conflict control), `SearchComparisonCoordinator` (search/comparison lifecycle, operation
  events, result paging, sync-plan generation/application), and `ActionInvoker` (core/plugin action
  discovery, context validation, platform dispatch, mutation submission). `FileManagerService`
  now owns these services and delegates through their small interfaces; its production portion is
  ~1,336 lines, with the remainder of `service.rs` containing facade-level integration tests. Added
  four interface tests across the new modules. Verified the 272-test `fm-application` unit suite,
  warning-free package typecheck, clean repository lint, and a dedicated code-review pass with no
  findings. The full repository test command passed all 1,326 Rust tests, doctests, and frontend
  tests; its script-test stage passed 38/40, with two unrelated pre-existing stale assertions:
  `ci-workflow.test.mjs` still requires the former `cargo test --workspace` CI command, and
  `desktop-packaging.test.mjs` still requires the former `dev.fm.desktop` bundle identifier.
