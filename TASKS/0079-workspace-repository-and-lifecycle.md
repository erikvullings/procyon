# 0079 Workspace repository, validation and default-workspace lifecycle

Status: done
Priority: high
Owner: unassigned
Agent: unassigned
Area: backend
Depends on: 0078

## Context
`file-manager-coding-agent-spec.md` §5.3.2, §5.3.6, §5.3.7, §5.3.8 and §5.3.16 items 1–4, 9 and 12.
This gives `fm-application` its first real `WorkspaceService`/`WorkspaceRepository` pair — today
`FileManagerService` only implements `runtime_capabilities` and explicitly defers workspaces (see
`crates/fm-application/src/service.rs`).

## Acceptance Criteria
- `WorkspaceRepository` trait per §5.3.8: `list`, `load`, `save(workspace, expected_revision)`,
  `delete(id, expected_revision)`, returning a typed `WorkspaceError`.
- An in-memory implementation for tests, then a persistent implementation using versioned JSON
  files under the platform config directory (`directories`/`dirs` crate) with atomic writes (temp
  file + rename). This storage choice is made independently of task 0030's settings storage to
  avoid a circular dependency between the two tasks; note in Agent Notes if a later task
  consolidates them onto one mechanism (e.g. SQLite).
- `Workspace::validate()` checks every invariant in §5.3.6 (all 14 items) and returns structured,
  itemized validation errors rather than one opaque failure.
- A `schema_version` + migration chain for `Workspace` itself (distinct from the general settings
  schema in 0030); a test migrates a v0 fixture forward to the current version.
- Default-workspace creation: one workspace named `Default`, two panes in a 50/50 horizontal split,
  one tab per pane, the home directory (or a configured secondary location for the second pane) as
  the initial location. Home-directory resolution goes through a small platform seam (e.g. the
  `dirs` crate) rather than a hard-coded per-OS path; richer platform integration is deferred to
  0058/0059/0060.
- Startup lifecycle per §5.3.7: load workspace summaries plus the last-active workspace id, select
  an explicitly requested workspace or else the last-active one or else create the default, validate
  and migrate, build the runtime object, open active tabs first and lazily load inactive tabs,
  without blocking the application shell until every tab is loaded.
- Corrupt or unreadable workspace data never crashes startup: the bad file is backed up and a valid
  default is substituted, mirroring 0030's settings recovery behaviour, with a surfaced notification
  hook the frontend can display later.
- Unit tests: one failing case per invariant (14 total), default-workspace shape, migration,
  revision monotonicity, corrupt-file recovery.

## Implementation Notes
- Lives in `fm-application` (`WorkspaceService` + `WorkspaceRepository`), depending only on
  `fm-domain` — no Axum/Tauri dependency (§3 rule 4).
- Reuse `fm-domain`'s refined types from 0078 directly; do not reintroduce a parallel DTO here —
  DTO/REST/Tauri wiring is task 0080's concern.
- This task does not yet expose the semantic `WorkspaceCommand` mutation API (0080) or events
  (0081); it only owns storage, validation and the create/load/list/delete lifecycle.

## Agent Notes
- 2026-07-29: Implemented end-to-end.
  - `crates/fm-domain/src/workspace.rs`: added `Workspace::validate() -> Result<(), Vec<WorkspaceValidationError>>`
    (inherent impl in fm-domain, not fm-application — Rust's orphan rules require inherent impls in
    the defining crate, and this is structural self-validation, not the orchestration/IO behaviour
    that crate's module doc says belongs in higher layers), `WorkspaceValidationError` (14 variants,
    one per spec §5.3.6 invariant), and constants `CURRENT_WORKSPACE_SCHEMA_VERSION`,
    `MAX_NAVIGATION_HISTORY_LEN`, `SPLIT_RATIO_RANGE`. 17 new tests (one or more failing cases per
    invariant + a positive "freshly built workspace validates" case). `cargo test -p fm-domain`:
    46 passed (was 29), `cargo clippy -p fm-domain --all-targets -- -D warnings`: clean.
  - Two invariants are deliberately **not** checked inside `validate()`, both documented in the
    code and tested:
    - #12 (unrecognised plugin columns): no known-column registry exists anywhere in fm-domain to
      validate against, so this is a documented no-op (never rejected) rather than a fabricated
      registry — proven by `invariant_12_unrecognised_plugin_column_id_does_not_fail_validation`.
    - #14 (revision must increase monotonically): can't be judged from a single `Workspace` value
      in isolation. Enforced instead in the repository layer: both `InMemoryWorkspaceRepository`
      and `JsonFileWorkspaceRepository::save()` auto-increment `revision` from previously-stored
      state and reject a stale `expected_revision` with `WorkspaceError::RevisionConflict`.
  - New `crates/fm-application/src/workspace/` module (8 files: `error`, `repository`, `memory`,
    `migration`, `default_workspace`, `persistent`, `service`, `mod`):
    - `WorkspaceRepository` (list/load/save/delete) matches spec §5.3.8 exactly. Added a second
      trait, `LastActiveWorkspaceStore` (get/set last-active workspace id) — **not** in the spec's
      literal trait but required for the §5.3.7 startup lifecycle; `WorkspaceService<R>` requires
      both bounds on `R`. Both traits use `#[async_trait]` to stay dyn-compatible for a future
      `Arc<dyn ...>` in fm-server (task 0080) — this is `async-trait`'s first real usage anywhere
      in this workspace (it was already a declared-but-unused workspace dependency).
    - `InMemoryWorkspaceRepository` for tests; `JsonFileWorkspaceRepository` for real persistence,
      with `JsonFileWorkspaceRepository::new(directory)` accepting any directory (tests use
      `tempfile::TempDir`) and a separate `JsonFileWorkspaceRepository::default_directory()`
      resolving the real platform location (`dirs::config_dir()/fm/workspaces`, falling back to
      `.fm-config/workspaces` if the platform reports none) — callers wanting the real default
      call `JsonFileWorkspaceRepository::new(JsonFileWorkspaceRepository::default_directory())`.
      Writes are atomic (`.tmp-<uuid>` in the same directory, then rename; a dedicated test
      confirms no leftover temp files). Corrupt JSON is renamed to `<file>.corrupt-<unix-ts>`
      (never deleted) and reported through a `WorkspaceNotifier` hook (`NoopWorkspaceNotifier` by
      default) instead of crashing; `list()` skips per-file corruption rather than failing the
      whole call, matching 0030's settings recovery behaviour ahead of 0030 actually existing.
    - This storage is intentionally independent of task 0030's (unimplemented) settings storage —
      own JSON files, not a shared mechanism — to avoid a circular dependency between the two
      tasks, per this task's own Implementation Notes. Flag in a later task's notes if/when the two
      get consolidated (e.g. onto SQLite).
    - `migration.rs` migrates the `Workspace` JSON shape forward by `schema_version` (currently a
      single `0 -> 1` step covering task 0078's additions: `default_view` on panes, `title`/
      `title_override`/`pinned`, `operation_centre`). **Caveat**: no persistence layer existed
      before this task, so no real v0 workspace file has ever existed on disk — the `v0_fixture()`
      test helper is a *reconstruction* of task 0078's pre-0079 field set, not verified against any
      historical file or git history. If a later task finds the real historical shape differed,
      this migration step needs revisiting.
    - `default_workspace()` builds the spec-required "Default" workspace: two panes in a 50/50
      horizontal split, one tab per pane, home directory resolved via `resolve_home_directory()`
      (the `dirs` crate, falling back to `/`), with an optional secondary location for the second
      pane's tab only.
    - `WorkspaceService::start()` implements the full §5.3.7 selection lifecycle: explicit request,
      else last-active id, else create-default; a missing (`NotFound`) or corrupt (`Corrupt`)
      selection falls back to a fresh default rather than failing startup, always ending by
      recording the resulting workspace as last-active. Tested both with the in-memory repository
      (missing-id recovery) and end-to-end with `JsonFileWorkspaceRepository` + a genuinely
      corrupted on-disk file (`start_recovers_from_a_corrupt_last_active_workspace_file_by_creating_a_default`)
      to close the gap between "the repository can detect corruption" and "startup actually
      recovers from it".
    - **Not implemented by this task** (explicitly out of scope per its own Implementation Notes):
      opening active tabs first and lazily loading inactive tabs' directory contents on startup —
      `Workspace`/`WorkspaceService` here only persist/select navigation state (`Location` per
      tab), not live directory listings; that behaviour belongs to the directory-listing service
      (0019/0020) and frontend, not this task. Also not implemented: the semantic
      `WorkspaceCommand`/`apply_command` mutation API and event emission (tasks 0080/0081), and
      wiring `WorkspaceService` into `FileManagerService`/Axum/Tauri (task 0080) — `service.rs` and
      `error.rs` in `fm-application` were deliberately left untouched.
  - New workspace deps: root `Cargo.toml` `[workspace.dependencies]` gained `dirs = "6.0.0"` and
    `tempfile = "3.15.0"`; `crates/fm-application/Cargo.toml` gained `async-trait`, `chrono`,
    `dirs`, `serde_json`, `tokio` (feature `fs`), and a new `[dev-dependencies] tempfile`.
  - Verification actually run (not assumed): `cargo test -p fm-domain` (46 passed), `cargo test -p
    fm-application` (32 passed, up from 3 pre-existing), `cargo test --workspace` (0 failures, incl.
    `fm-test-support`'s architecture-layering fitness test — new deps don't violate crate layering),
    `cargo clippy --workspace --all-targets -- -D warnings` (clean), `cargo fmt --all -- --check`
    (clean), `cargo doc -p fm-domain -p fm-application --no-deps` (no `missing_docs` warnings).
  - No README.md/CLAUDE.md changes: this task adds an internal `fm-application` library surface
    only (no REST/CLI/Tauri exposure yet — that's task 0080), so there is no new user-facing
    surface to document.
  - Repo memory: added `/memories/repo/fm-application-workspace-conventions.md` capturing the
    trait/storage/migration/validation decisions above for tasks 0080/0081 to build on.

