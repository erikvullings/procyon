# 0086 F4 edit-in-external-editor action

Status: done
Priority: medium
Owner: unassigned
Agent: unassigned
Area: cross-cutting
Depends on: 0058

## Context
Follow-up from the same regression-report conversation as task 0085: the user expects the footer's
function-key bar to read F2 Rename, F3 View, F4 Edit, F5 Copy, F6 Move, F7 New Folder, F8 Delete (Total
Commander convention). No `core.edit` action exists anywhere yet (backend registry
`crates/fm-application/src/action.rs`, mock fixture `fixtures/mock-responses/actions.json`, or
frontend) — F4 is currently unbound. The footer's `footerFunctionKeyBindings` helper
(`frontend/src/keybindings/dispatcher.ts`) already accepts F2/F4/F5/F6/F7/F8 and sorts ascending,
so once `core.edit` is registered with a default `F4` shortcut it will appear in the footer for
free, in the right slot, with no further footer changes needed.

Confirmed with the user: F4 should open the selected file in the system's default text editor
(Total-Commander-style "Edit"), and F2 Rename should remain in the footer alongside it.

## Acceptance Criteria
- New `core.edit` action registered in `crates/fm-application/src/action.rs` (`core_action(...)`
  helper), category `fileOperations`, default shortcut `F4`, single-selection requirement (same
  shape as `core.open`/`core.rename` — see `capability_gated_single_selection`).
- Add `core.edit` to `fixtures/mock-responses/actions.json` with `contextRequirements: {}` (mock
  fixture convention — see other entries) and title `Edit`.
- Decide and implement the actual invocation: reuse
  `PlatformAdapter::open_with_default_application`-style dispatch, but launching a **text editor**
  specifically rather than the file's default association (e.g. opening a `.jpg` with F4 should
  still open a text/hex editor, not Preview/Photos). Two reasonable approaches, pick one:
  1. Add a `editor_command: Option<String>` field to `fm-settings::Settings` (same pattern as the
     existing `terminal_command: Option<String>` used by `core.openTerminal`), launched with the
     selected file's path as an argument; `None` falls back to the platform's default text-editor
     association if one is easy to resolve, otherwise falls back to
     `open_with_default_application` (documented, non-ideal fallback — matches the existing
     documented gap for `core.openWith`).
  2. Add a new `PlatformCapabilities::EDIT_IN_TEXT_EDITOR` capability + adapter method mirroring
     `open_terminal`'s pattern (each platform crate supplies its own "open in $EDITOR" logic, e.g.
     `$VISUAL`/`$EDITOR` env var on macOS, associated text editor via `notepad.exe` fallback on
     Windows). More correct but larger; only pursue if approach 1's fallback proves unsatisfying.
- Frontend: no footer/keybinding-dispatcher changes needed (already accepts F4, see Context) — just
  confirm via a test that `core.edit` shows up in the footer once registered.
- OpenAPI/transport DTO regen if `core.edit`'s parameter schema or settings changes require it
  (`pnpm run api:export` / `api:generate` — see `AGENTS.md` "Generated code").
- Tests: backend action-registry test asserting `core.edit`'s shortcut/requirements, an
  invocation-dispatch test (mocked editor launch), and a frontend dispatcher test asserting F4
  appears between F2 and F5 in `footerFunctionKeyBindings` output once a `core.edit`-shaped action
  is present in the fixture list.

## Implementation Notes
- `crates/fm-application/src/action.rs`'s doc comment on `core_actions` explains the
  `capability_gated_*` helpers and the existing documented gap for `core.openWith` — follow the
  same "documented gap over silent over-claim" convention here if the editor-launch fallback is
  imperfect on a given platform.
- `crates/fm-settings/src/lib.rs`'s existing `terminal_command: Option<String>` field + its
  settings-editor UI wiring (task 0083) is the closest precedent for exposing `editor_command` to
  users, if approach 1 above is chosen.

## Agent Notes
- Implemented approach 1 from this task's own Implementation Notes: added
  `editor_command: Option<String>` to `fm-settings::Settings` (+ `SettingsDto`), mirroring the
  existing `terminal_command` field, and a settings-editor "Editor command" text field.
- `PlatformAdapter::open_in_text_editor(path, command_override)` is a new trait method with a
  default impl that falls back to `open_with_default_application` (the documented, non-ideal
  fallback this task anticipated). It reuses `PlatformCapabilities::OPEN_WITH_DEFAULT_APPLICATION`
  — no new capability bit was added, per the task's approach-1 guidance.
  - macOS gets a real implementation: `open -t <path>` (system default text editor) with no
    override, or `open -a <override> <path>` when `editor_command` is set.
  - Windows continues to delegate 100% to the fallback adapter (task 0060 still tracks real
    Explorer/Windows-native integration), matching its existing exhaustive-delegation style.
- `core.edit` registered in `crates/fm-application/src/action.rs`: category `fileOperations`,
  shortcut `F4`, gated by `capability_gated_single_selection(open_available)` (same shape as
  `core.open`). Dispatches through a new `PlatformActionKind::EditInTextEditor` arm in
  `crates/fm-application/src/service.rs`'s `invoke_platform_action`, which reads `editor_command`
  from settings and forwards it as the override.
- Added `core.edit` to `fixtures/mock-responses/actions.json` (title `Edit`, shortcut `F4`,
  `contextRequirements: {}`), and to the `mock-file-manager-client.test.ts` action-id-order
  assertion (the mock fixture's order is hand-maintained, not alphabetically derived — both must
  stay in sync).
- Bundled with this task (per the user's request, not part of either task's original Acceptance
  Criteria): registered a Marta-style `Ctrl+Enter` (translated to `Cmd+Enter` on macOS via the
  existing `primary()` shortcut helper) shortcut on `core.openWith`, wired through
  `app-shell.ts`'s `handleGlobalKeydown` — `pane.ts`'s local keydown handler leaves
  `core.view`/`core.edit`/`core.openWith` unhandled and un-`stopPropagation`'d, so the event
  bubbles to the document-level handler, which now resolves the active pane's single selection and
  invokes the action with a `{ uri }` parameter, reusing the same pattern as `core.copy`/`core.move`/
  `core.trash`.
- Verified: full Rust workspace `cargo test`/`cargo clippy -D warnings`/`cargo fmt --check` and the
  full frontend `tsc --noEmit`/`vitest run` (465 tests) all pass. Three pre-existing, unrelated
  failures were independently confirmed via `git stash` + rerun and left untouched (see
  `/memories/repo/fm-cargo-sandbox-target-dir.md`): `fm-server`'s
  `plugin_routes::list_plugins_starts_empty_and_unknown_enablement_is_not_found`, `fm-vfs-local`'s
  `metadata_is_separate_and_capabilities_are_truthful`, and `fm-plugin-runtime`'s
  `discovers_the_real_catppuccin_icons_plugin_package`.
- `frontend/openapi/openapi.json` and `frontend/src/api/generated/models/settingsDto.ts` were
  regenerated via `pnpm run api:export && pnpm run api:generate` (never hand-edited) to add the new
  optional `editorCommand` field.
