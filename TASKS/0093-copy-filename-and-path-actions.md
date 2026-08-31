# 0093 Copy filename and path actions

Status: done
Priority: medium
Owner: unassigned
Agent: unassigned
Area: frontend
Depends on: none

## Context

QA request: "We need to build a command for exporting filename with or without
path." Two action ids are already reserved in the core action registry
(`crates/fm-application/src/action.rs`, `core_actions()`) but documented as
permanently unavailable pending this work:

- `core.copyPath` - copy the selected entry's full path to the system
  clipboard.
- `core.copyRelativePath` - copy the selected entry's path relative to the
  current directory/workspace root.

Neither has a frontend handler today (`app-shell.ts` has no
`dispatchedAction === 'core.copyPath'` branch); they are listed in
`frontend/src/features/commands/availability.ts` as known action ids only, so
they render as permanently disabled in the command palette/context menu.

Additionally, the user wants a plain "filename only" (no path) variant, which
does not have a reserved action id yet - this task should introduce
`core.copyName` alongside wiring the two existing reserved ids.

`plugins/sample-copy-markdown-path/` (task 0055) is a related, existing
sample plugin that copies a path as a Markdown link - useful prior art for
system-clipboard access from the frontend, but this task is about first-party
core actions, not the plugin.

## Acceptance Criteria

- `core.copyName` (new), `core.copyPath`, and `core.copyRelativePath` are
  wired to a frontend handler that writes to the system clipboard
  (`navigator.clipboard.writeText`, with a Tauri-safe fallback if needed) for
  the current selection (single or multi-select: join multiple with
  newlines).
- `core.copyPath` copies the absolute/full location (with filename).
  `core.copyRelativePath` copies the path relative to the active directory's
  root or workspace root (confirm which with existing relative-path
  conventions elsewhere in the codebase). `core.copyName` copies just the
  entry's filename, no path.
- Backend `ActionContextRequirements.feature_available` flips to `true` for
  these three ids once implemented (update `core_actions()` and its doc
  comment in `crates/fm-application/src/action.rs`).
- Keybindings/menu entries follow the existing action-registration
  conventions (command palette, context menu availability).
- Tests: frontend unit tests for the clipboard-writing handler(s), and any
  backend action-registry tests updated for the new availability.

## Implementation Notes

- Frontend dispatch lives in `frontend/src/app/app-shell.ts` alongside the
  other `dispatchedAction === 'core.xxx'` branches (e.g. `core.copy`,
  `core.move`, `core.trash`).
- `frontend/src/features/commands/availability.ts` already lists
  `core.copyPath` / `core.copyRelativePath` as known ids; add `core.copyName`
  there too.
- Reference: `crates/fm-application/src/action.rs` lines ~160-200 doc comment
  explaining why these ids were deferred.

## Agent Notes

- Created in response to live QA feedback (round 3): "In the file manager,
  you should not be able to select text with the cursor... We need to build
  a command for exporting filename with or without path." The text-selection
  part of that request was fixed directly (see
  `frontend/src/features/directory-table/directory-table.css`,
  `user-select: none` on `.fm-directory-table`); this task tracks the
  filename/path export command itself, which was not implemented as part of
  that fix.
- 2026-08-06 Codex: Added available `core.copyName`, `core.copyPath`, and
  `core.copyRelativePath` registry actions and frontend handlers that copy
  newline-separated multi-selections to the system clipboard. Full paths use
  the existing decoded location-path display convention; relative paths are
  rooted at the active directory. Added a Clipboard API implementation with a
  document-copy fallback for WebViews, updated mock actions/context-menu
  availability, and documented the behavior. Verified 5 task-specific tests
  (3 clipboard formatting/writing, 1 menu-availability, 1 action-registry),
  `pnpm --dir frontend typecheck`, `pnpm run lint`, and `pnpm test`.
