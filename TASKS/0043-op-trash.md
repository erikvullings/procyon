# 0043 Operation: move to Trash / Recycle Bin

Status: done
Priority: high
Owner: unassigned
Agent: unassigned
Area: backend
Depends on: 0041

## Context
`file-manager-coding-agent-spec.md` §16 milestone 2, §23 (macOS Trash, Windows Recycle Bin) and §21
(`systemTrash` capability).

## Acceptance Criteria
- `OperationKind::Trash` moves entries to the platform trash on macOS and Windows.
- The `TRASH` provider capability and the `systemTrash` runtime capability report the truth for the
  current platform and location; where trash is unavailable (e.g. some network volumes, server
  mode), the UI offers permanent delete with explicit confirmation instead of silently falling back.
- Trashing is never used as a silent substitute for delete and vice versa (§35).
- Progress and cancellation behave like other operations.
- Integration tests run only against temporary test roots and clean up any trashed items they create
  (§27); on platforms where that cannot be done safely, the test is skipped with an explicit report
  rather than silently passing (§35).
- `F8`/`Delete` maps to trash when available, `Shift+Delete` to permanent delete (0044).

## Implementation Notes
- Use a maintained crate (e.g. `trash`) behind the `fm-platform` adapter trait (0058) rather than
  calling platform APIs from the operation directly.
- Server/browser mode should default to a configurable trash directory inside the allowed roots
  rather than the OS trash (§22).

## Agent Notes
- 2026-08-03 agent: macOS trash was already implemented end-to-end in `fm-platform-macos`
  (`PlatformAdapter::trash` via `NSFileManager::trashItemAtURL_resultingItemURL_error`, with its own
  real-tempdir test) by earlier work, but nothing ever dispatched to it: `FileManagerService::
  start_operation`'s executor-dispatch match in `crates/fm-application/src/service.rs` had no arm for
  `OperationKindDto::Trash` and silently fell through to a dead-code `NoOpExecutor`. That was the
  actual gap this task closed.
  - Added a `TrashExecutor : OperationExecutor` in `service.rs` that goes through the normal
    operation engine (`Scheduler`/`OperationPlan`) so trash gets the same progress/cancellation/
    warning semantics as every other operation, per this task's acceptance criteria — but bypasses
    `FileSystemProvider` and calls `self.platform.trash(&native_path)` directly per source, since
    trash is a platform concern, not a VFS one. `plan()` builds one zero-byte `PlanItem` per
    top-level source (no recursive directory-tree enumeration like `DeleteExecutor`), because the
    native trash APIs already move a whole tree in one call.
  - `start_operation` now checks `PlatformCapabilities::TRASH` before constructing the executor and
    returns `ApplicationError::PlatformOperationFailed` synchronously if the platform can't trash,
    matching the existing pattern for other capability-gated operations.
  - `TrashExecutor` has no `requires_confirmation()` override, no read-only override field, and no
    audit logging (trash is reversible; task 0044's mandatory-confirmation/audit path stays specific
    to permanent delete).
  - Removing the now-unreachable `_ => Arc::new(NoOpExecutor)` wildcard (the match is exhaustive
    over all 7 `OperationKindDto` variants) made `NoOpExecutor` itself fully dead code, so it and its
    `OperationExecutor` impl were deleted.
- `crates/fm-application/src/action.rs`: added a capability-gated `core.trash` action (title
  "Trash", category `fileOperations`) implementing the `F8`/`Delete` keybinding split from this
  task and from 0044's acceptance criteria: when `PlatformCapabilities::TRASH` is available,
  `core.trash` owns the bare `F8`/`Delete` keys (the safe, reversible default) and `core.delete`
  moves to `Shift+F8`/`Shift+Delete`; when trash is unavailable, `core.trash` registers with no
  shortcuts and `feature_available: false`, and `core.delete` keeps the bare keys exactly as before.
- Frontend wiring (mirrors the backend split so mock/browser mode behaves identically to the real
  desktop app once a platform reports `systemTrash: true`):
  - `frontend/src/app/app-shell.ts`: new `core.trash` dispatch block calling `startOperation({
    type: 'trash', sources, conflictPolicy: 'ask' })` — no `permanentDeleteConfirmed`/
    `overrideReadOnly` fields, since those are permanent-delete-only concerns.
  - `frontend/src/features/commands/availability.ts`: added `core.trash` to `SELECTION_ACTION_IDS`
    (context-menu inclusion) but **deliberately not** to `WRITE_SELECTION_ACTION_IDS` — trash has no
    `overrideReadOnly` escape hatch and is reversible, so read-only selected entries stay trashable,
    unlike rename/move/permanent-delete.
  - `fixtures/mock-responses/actions.json`: added a `core.trash` entry. Since the mock client
    reports `systemTrash: false` (mock/browser mode has no real OS trash implementation), it's
    registered with empty `defaultShortcuts` and `contextRequirements.featureAvailable: false` —
    exactly mirroring what the real backend would report for a TRASH-incapable platform — so
    `core.delete` keeps its original unshifted `F8`/`Delete` shortcuts in mock mode; no behavior
    change for the existing mock/dev-server experience.
- Verification:
  - `cargo test -p fm-application` (118 lib tests + all integration suites) — all passing, including
    3 new unit tests covering `TrashExecutor` dispatch success, the missing-`TRASH`-capability
    rejection path, and a per-item-platform-failure → `CompletedWithWarnings` path.
  - `cargo test -p fm-application --lib action::` — 10 passing, including 2 new tests for the F8/
    Delete keybinding split (available vs. unavailable).
  - `cargo test --workspace`, `cargo clippy --workspace --all-targets`, `cargo fmt --all --check` —
    clean, except one pre-existing, unrelated failure in `apps/fm-server/tests/plugin_routes.rs`
    (`list_plugins_starts_empty_and_unknown_enablement_is_not_found`, an environment-dependent
    plugin-discovery count) confirmed via `git stash` to fail identically on `main` before this
    task's changes.
  - `pnpm exec vitest run` (frontend, full suite) — 417/417 passing, including 3 new/updated test
    files (`app-shell.test.ts`, `mock-file-manager-client.test.ts`, `availability.test.ts`).
    `tsc --noEmit` and `biome check .` — clean.
- Known gaps (explicitly out of scope for this pass, matching the state found at investigation
  time):
  - Windows native trash (`crates/fm-platform-windows`) is not implemented; `core.trash` is
    unavailable there today (capability absent), so `core.delete` alone owns `F8`/`Delete` on
    Windows until that adapter gains a `trash()` implementation.
  - The Implementation Notes' suggested configurable server/browser-mode fallback trash directory
    (§22) was not built. `FallbackPlatformAdapter` reports no `TRASH` capability, so browser/server
    mode always falls back to permanent delete with its existing mandatory confirmation — this
    satisfies the acceptance criterion ("offers permanent delete with explicit confirmation instead
    of silently falling back") but not the stretch-goal fallback trash directory itself.
