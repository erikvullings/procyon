# 0062 Drag and drop within the app and with the OS

Status: in_progress
Priority: low
Owner: unassigned
Agent: unassigned
Area: desktop
Depends on: 0061, 0048

## Context
`file-manager-coding-agent-spec.md` §15 (table is a drag source and drop target), §23 (drag to and
from Finder/Explorer) and §33 step 10.

## Acceptance Criteria
- Within the app: dragging a selection between panes and tabs starts a copy or move operation
  through the engine (modifier decides; default documented per platform).
- Drop targets highlight clearly, invalid targets are rejected before the drop, and dropping onto a
  directory row targets that directory rather than its parent.
- Native drag-out to Finder/Explorer and drag-in from them, capability-gated via `nativeDragOut`
  (§21) and unavailable in browser mode.
- Dropping files from outside the app starts the appropriate operation with the same conflict and
  confirmation rules as any other operation (§35 — no silent overwrite).
- Keyboard-accessible alternatives exist for everything drag can do (§29).
- Drag of a very large selection does not stall the UI.
- Tests: drop-target resolution and validation logic (unit); native drag verified manually per
  platform and recorded in the task notes.

## Implementation Notes
- Native drag-out requires platform work behind the adapter traits (0058); if a platform cannot be
  supported yet, report the capability as `false` rather than half-implementing it.

## Agent Notes
- 2026-08-08 Codex: Implemented typed in-app drag/drop for selections between panes and onto loaded
  tabs. Directory rows resolve to themselves while files/empty space resolve to the containing
  directory; unavailable, read-only, same-location, and source-subtree targets are rejected during
  `dragover`, and valid targets receive a visible outline. Accepted drops start ordinary `copy` or
  `move` operations with `conflictPolicy: ask`, so drag uses the same conflict/confirmation engine
  as clipboard paste and never mutates files in TypeScript. Move is the documented default;
  Option requests copy on macOS and Control requests copy elsewhere. Existing Ctrl/Cmd+C/X/V is
  the keyboard-accessible equivalent. Drag payloads use one small internal marker rather than
  serializing the selection through `DataTransfer`; source locations stay in frontend state.
  Added 5 task-specific Vitest cases: 3 drop resolution/validation/modifier tests, 1 table event
  test, and 1 cross-pane operation-dispatch test. The three task test files pass (118 tests total
  in those files, including 5 attributable to this task).
  Native OS drag-in/out is deliberately not half-implemented: every current platform adapter still
  reports `nativeDragOut: false`, so it is unavailable in browser mode and all current desktop
  builds per the Implementation Notes. Manual platform verification therefore records macOS,
  Windows, and Linux as unavailable rather than falsely claiming an interactive native test.
  Full frontend Vitest: 692 passed, 1 skipped, with three pre-existing failures unrelated to this
  task (CodeMirror viewer mount timing, stale mock action list, and a CSS whitespace-string test).
  Typecheck has no new errors; three pre-existing errors remain in archive optional-property test
  data/configuration. `git diff --check` is clean. No CLAUDE.md exists; AGENTS.md needed no change
  because no development contract changed. README documents the new user-facing behavior.
- 2026-08-08 Codex follow-up: Enabled native file-reference drag-in/out in macOS and Windows Tauri
  builds. Both platform adapters now advertise `nativeDragOut`; Linux, browser, and mock adapters
  remain capability-disabled. Drag-out validates provider-neutral locations as local native paths
  before handing them to `drag-rs` on Tauri's main thread. Tauri window drop events are converted
  back into validated `Location` values and dispatched through the existing drop validation and
  operation engine as conflict-safe copies. Added typed desktop errors, teardown-safe frontend
  subscription handling, and tests for empty/non-local selections, awkward path round trips,
  Tauri invocation/path conversion, selection handoff, and incoming operation dispatch.
  `cargo test -p fm-desktop` passes (8 tests), focused native frontend tests pass, relevant Biome
  checks pass, and Clippy passes for `fm-desktop` and `fm-platform-macos`. Full frontend typecheck
  still has the same three unrelated pre-existing errors recorded above. A Windows cross-check
  reaches target-specific compilation but cannot complete on this Mac because the MSVC C headers
  and toolchain are absent (`lzma-sys`/`mlua-sys`). Interactive Finder and Explorer drag tests have
  not been performed in this non-interactive environment, so this task is returned to
  `in_progress` until both manual platform checks are recorded.
- 2026-08-19 Claude: Fixed a bug found during manual verification: on desktop builds with
  `nativeDragOut` enabled (macOS, Windows), every in-app drag — even a plain move between panes on
  the same volume — was routed through the native OS drag session, and the drop handler forced
  `copy` unconditionally for any drop that came back through that native round-trip, regardless of
  modifier keys. In-app drags could never resolve to `move`. Root cause: `onDrop`/`onTabDrop` in
  `frontend/src/features/workspace/pane-content-builder.ts` short-circuited on
  `context.getNativeDropInProgress()` before consulting `operationForDrop`, and that flag was set
  for every native-routed drop, not just genuine external ones. Fix: added a
  `nativeDragSourceInternal` flag (set in `onDragStart` when a drag starts inside the app) and a
  location-set comparison in `workspace-controller.ts`'s `subscribeNativeFileDrops` handler; the
  drop is now only treated as external (forcing `copy`) when its locations don't match what this
  window itself started dragging. In-app drags on native-drag-capable platforms now default to
  `move`, matching non-native/browser builds; genuine drag-in from Finder/Explorer still forces
  `copy` (existing behavior, still covered by the pre-existing "copies a native file drop" test).
  Added a Vitest case ("defaults an in-app drag to move even when routed through the native drag
  host") verifying `startOperation` receives `type: 'move'` for a same-window native-routed drag.
  Full frontend Vitest: 1352 passed, 0 failed. `tsc --noEmit` clean. Manual Finder/Explorer
  interactive drag tests still not recorded in this non-interactive environment; task remains
  `in_progress` for that reason alone.
