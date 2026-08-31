# 0087 F3 view action

Status: done
Priority: medium
Owner: unassigned
Agent: unassigned
Area: cross-cutting
Depends on: 0058

## Context
Same footer-completeness follow-up as 0086. Total Commander convention reserves F3 for "View".
`footerFunctionKeyBindings` (`frontend/src/keybindings/dispatcher.ts`) already accepts F2/F3/F4/
F5/F6/F7/F8 sorted ascending, so registering `core.view` with a default `F3` shortcut makes it
appear in the footer in the right slot automatically — no footer changes needed.

For now, F3 View opens the selected file with the system's default application, i.e. behaves like
`core.open` (see `capability_gated_single_selection` / `PlatformCapabilities::
OPEN_WITH_DEFAULT_APPLICATION` in `crates/fm-application/src/action.rs`). This is an intentional
stopgap: task 0088 tracks a real in-app "Lister"-style viewer that F3 should switch to using once
it exists, without changing the shortcut, title, or footer wiring.

## Acceptance Criteria
- New `core.view` action registered in `crates/fm-application/src/action.rs`, category
  `fileOperations`, default shortcut `F3`, single-selection requirement (same shape as
  `core.open`).
- Add `core.view` to `fixtures/mock-responses/actions.json` (title `View`, `contextRequirements:
  {}`, matching mock-fixture convention).
- Invocation dispatch: reuse the existing `open_with_default_application` platform-adapter call
  used by `core.open` — no new platform capability needed for this task.
- Document in the action's description/doc comment that this is a stopgap and task 0088 is the
  real viewer.
- Tests: backend action-registry test for `core.view`'s shortcut/requirements/dispatch, and a
  frontend dispatcher test asserting F3 sits between F2 and F4 in `footerFunctionKeyBindings`
  output.

## Agent Notes
- `core.view` registered in `crates/fm-application/src/action.rs`: category `fileOperations`,
  shortcut `F3`, gated by `capability_gated_single_selection(open_available)` — identical shape to
  `core.open`. Dispatches via the existing `PlatformActionKind::Open` arm (maps `"core.view"` to
  `Open` in `platform_action_kind`, `crates/fm-application/src/service.rs`), i.e. it currently opens
  with the default application, exactly as this task's Context specifies as the intentional task
  0088 stopgap. No new platform capability was added.
- Added `core.view` to `fixtures/mock-responses/actions.json` (title `View`, shortcut `F3`,
  `contextRequirements: {}`) and to the `mock-file-manager-client.test.ts` action-id-order
  assertion (hand-maintained fixture order, kept in sync manually).
- Frontend: added `core.view` (alongside `core.edit`) to `platformActionParameters` and
  `SELECTION_ACTION_IDS` in `crates/../frontend/src/features/commands/availability.ts` so it's
  treated identically to `core.open` for command-palette/context-menu invocation and availability
  gating.
- Bundled with tasks 0086/0087 together (per the user's explicit request): a Marta-style
  `Ctrl+Enter`/`Cmd+Enter` shortcut on `core.openWith` — see task 0086's Agent Notes for the full
  design/verification writeup (shared implementation across both tasks).
- Verified: full Rust workspace `cargo test`/`cargo clippy -D warnings`/`cargo fmt --check`, and
  full frontend `tsc --noEmit`/`vitest run` (465 tests) pass. See task 0086's Agent Notes for the
  list of independently-confirmed pre-existing unrelated failures left untouched.
