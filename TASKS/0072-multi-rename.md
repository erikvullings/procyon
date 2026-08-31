# 0072 Multi-rename tool

Status: done
Priority: low
Owner: unassigned
Agent: unassigned
Area: frontend
Depends on: 0038, 0051

## Context
`file-manager-coding-agent-spec.md` §16 milestone 3 (multi-rename with preview) and §37.

## Acceptance Criteria
- Entry point: pressing F2 (`core.rename`) with more than one entry selected opens this dialog
  instead of the single-entry inline rename input (see `beginRename` in
  `frontend/src/features/panes/pane.ts`, which currently only implements the single-selection
  inline-input path); F2 with exactly one entry selected keeps using the existing inline rename.
- A multi-rename dialog operating on the current selection with rules for: search and replace,
  prefix, suffix, sequence number (configurable start/step/padding) and case transformation.
- A live preview table shows old name → new name for every selected entry before anything is
  applied (§16).
- Collisions (with each other or with existing files) are detected and highlighted in the preview,
  and applying is blocked until resolved or the conflict policy is chosen.
- Names that are invalid on the target platform are flagged in the preview.
- Applying runs through the operation engine as rename operations — never direct filesystem calls
  from the UI (§35) — and is a single cancellable operation with progress.
- Case-only renames work (reuses 0038's handling).
- The rename plan can be reviewed and cancelled; nothing is applied until confirmed.
- Vitest tests for each rule and for collision detection; Rust integration test for applying a plan.

## Implementation Notes
- The rule engine is a pure function `(entries, rules) → proposed names` so it is fully unit-tested
  and reusable by the "uppercase rename preview" sample plugin (§20 optional plugin 3).
- Regex search/replace should be opt-in and validated before use.

## Agent Notes
- Implemented end-to-end.
- Backend (`crates/fm-transport-dto`, `crates/fm-application`):
  - `StartOperationRequestDto`/`StartOperationRequest` gained an optional `destinations:
    Vec<Location>` field (one entry per `sources` item), used only for batch rename; every other
    operation kind and single-entry rename keep using the existing `destination` field.
  - `FileManagerService::start_operation`'s `Rename` handling now branches: with no `destinations`
    it behaves exactly as before (single-entry `RenameExecutor`); with `destinations` populated it
    validates `sources.len() == destinations.len()`, builds one `RenameExecutor` per pair (same
    provider/cross-provider/capability checks as the single-entry path), and wraps them in a new
    `RenameGroupExecutor` so the whole batch runs as a single cancellable operation with progress,
    never falling back to copy+delete. Case-only renames reuse the existing per-item
    `RenameExecutor`, so 0038's case-only handling works unchanged in batch mode too.
  - New integration tests in `crates/fm-application/tests/rename_operation.rs`:
    `renames_multiple_entries_in_one_batch_operation` and
    `batch_rename_collision_fails_without_overwriting_other_entries` (asserts a colliding batch
    fails without corrupting unrelated entries).
- OpenAPI/Orval regenerated (`frontend/openapi/openapi.json`,
  `frontend/src/api/generated/models/startOperationRequestDto.ts`) and the hand-written
  `StartOperationRequest` model/HTTP client adapter updated to match.
- Frontend rule engine: `frontend/src/features/operations/multi-rename-rules.ts` — pure functions
  (`proposeRenames`, `applySearchReplace`, `applyCaseTransform`, `formatSequence`,
  `canApplyRenamePlan`, `validateSearchPattern`) covering search/replace (regex opt-in, validated),
  prefix/suffix, sequence numbering, case transformation (unchanged/upper/lower/title — title-case
  leaves the extension casing untouched, upper/lower transform the whole name), and
  case-insensitive collision detection against both the rest of the batch and existing sibling
  entries. 27 Vitest tests in `multi-rename-rules.test.ts`.
- Dialog UI: `frontend/src/features/operations/multi-rename-dialog.ts` (ModalPanel-based, live
  preview table with old→new columns, per-row collision/invalid-name highlighting, Rename button
  disabled until the whole plan is collision- and error-free). 7 Vitest tests in
  `multi-rename-dialog.test.ts`. CSS added to `frontend/src/themes/theme.css`.
- Wiring: `Pane.beginRename` (`frontend/src/features/panes/pane.ts`) now opens the multi-rename
  dialog via a new optional `onMultiRename` attr when more than one entry is selected, otherwise
  keeps the existing single-entry inline rename; `WorkspacePaneContent`
  (`frontend/src/features/workspace/workspace-layout.ts`) forwards the new callback;
  `frontend/src/app/app-shell.ts` owns the dialog's open/entries/existing-sibling-names state and,
  on apply, calls `client.startOperation({ type: 'rename', sources, destinations, conflictPolicy:
  'ask' })` with one destination per renamed entry.
- Verification: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`
  (clean), `cargo fmt --all --check` (clean), full frontend Vitest suite (68 files / 618 tests
  passing), `pnpm exec tsc --noEmit` (clean), Biome check on all touched files (clean).
- Known pre-existing failures unrelated to this task (see
  `/memories/repo/fm-application-workspace-conventions.md` and
  `/memories/repo/fm-cargo-sandbox-target-dir.md` for details, not introduced or fixed here):
  `fm-plugin-runtime::tests::discovers_the_real_catppuccin_icons_plugin_package` (stale fixture
  icon count), `fm-server`'s
  `plugin_routes::list_plugins_starts_empty_and_unknown_enablement_is_not_found`, and an
  occasionally-flaky `conflict_resolution.rs::a_destination_appearing_after_planning_is_resolved_
  like_an_initial_conflict` (timing-dependent, confirmed to flake on a clean HEAD checkout too).

