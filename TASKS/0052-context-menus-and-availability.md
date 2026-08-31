# 0052 Context menus and context-sensitive action availability

Status: done
Priority: medium
Owner: unassigned
Agent: unassigned
Area: frontend
Depends on: 0051

## Context
`file-manager-coding-agent-spec.md` §18 (menus, context menus, toolbars and shortcuts all invoke the
same registry) and §33 step 8.

## Acceptance Criteria
- Right-click (and the keyboard context-menu key) on the directory table opens a menu built from the
  action registry for the current selection.
- Menu contents adapt to context: no selection, single file, single directory, multiple entries,
  read-only location — with unavailable actions hidden or disabled consistently.
- Menus are keyboard navigable with correct focus return, using `mithril-materialized` menus (§14).
- Empty-area right-click offers location actions (create directory, paste, refresh, open terminal).
- The same availability evaluation is used by the palette, the function-key bar and the menus — one
  implementation, not three.
- Frontend availability evaluation is advisory only; the backend re-validates on invoke (§18).
- Vitest tests cover menu composition per context.

## Implementation Notes
- Keep the availability predicate pure and shared in `features/commands/`.
- Native menus (macOS/Windows menu bar) are task 0059/0060; this is the in-window context menu.

## Agent Notes
- Implemented a pure, shared command-availability evaluator for the command palette, function-key
  bar, and in-window context menu. The menu supports pointer and keyboard invocation, selected-entry
  and empty-location composition, disabled unavailable entries, and focus restoration.
- Added `core.paste` and `core.refresh` to the application registry and aligned the mock registry.
  Paste and create-directory operations retain the pane/location where the menu was opened; the
  operation endpoint remains the authoritative backend validation boundary.
- `Open Terminal Here` is displayed but disabled when the host/runtime reports no terminal support;
  native terminal integration remains unavailable in the current mock/browser capability set.
- Verified with `pnpm run lint` and `pnpm test` (including 276 passing frontend tests and one
  existing skipped test).
