# 0028 Selection model and keyboard navigation

Status: done
Priority: high
Owner: unassigned
Agent: unassigned
Area: frontend
Depends on: 0027

## Context
`file-manager-coding-agent-spec.md` §15 ("keyboard behaviour") and §27 (the selection reducer and
keyboard navigation are named frontend test targets). Selection is independent of the cursor.

## Acceptance Criteria
- A pure selection reducer in `features/selection/` supporting: cursor movement, single selection,
  range selection, discontinuous selection, select all, invert selection, and clearing.
- Selection survives sorting and filtering (keyed by `EntryId`), and is pruned when entries
  disappear via a delta.
- Keyboard bindings from §15 implemented for navigation and selection:
  `Up/Down`, `PageUp/PageDown`, `Home/End`, `Enter`, `Backspace`, `Tab`, `Space`,
  `Shift+Arrow`, `Ctrl/Cmd+A`.
- Type-to-select jumps to the first entry matching the typed prefix, with a timeout reset.
- Platform-appropriate modifiers: Command on macOS, Control on Windows/Linux (§29).
- Browser-reserved shortcuts that cannot be intercepted reliably are avoided or remapped (§15).
- Vitest tests cover every reducer transition and each key binding, including range selection across
  a sort change.

## Implementation Notes
- The reducer is pure and independent of Mithril so it is trivially testable (§27).
- Keybindings are hard-coded here but must route through the action system once 0050 lands; define
  the action ids now (`core.selectAll`, `core.invertSelection`, ...) to avoid rework.

## Agent Notes
- 2026-07-31 codex: Added a pure stable-`EntryId` selection reducer under
  `features/selection/` for independent cursor movement, single/range/discontinuous selection,
  select all, invert, clear, and explicit delta pruning. Range anchors remain stable across
  reordered visible entries, while filtered-out selections remain selected.
- 2026-07-31 codex: Added typed semantic key interpretation with reserved `core.*` action IDs,
  runtime-capability platform modifiers, viewport/edge movement, Shift+Arrow extension, Space
  toggling, Ctrl/Cmd+A, and 700 ms prefix type-to-select. Integrated it with per-pane AppShell
  state while preserving task 0027's Enter, Backspace, history, and workspace Tab behavior.
- 2026-07-31 codex: Verified 28 dedicated reducer/keybinding cases and 4 task-specific pane/AppShell
  integration cases by rerunning their five Vitest files. Frontend typecheck and production build,
  repository-wide formatting/clippy/Biome lint, and the full Rust/frontend/script `pnpm test`
  suite pass. No `CLAUDE.md` exists to update; README documents the added keyboard and selection
  surface.
- 2026-07-31 codex: Follow-up adds a timed footer prefix, red prefix highlights on every visible
  match, and Arrow/Page/Home/End navigation constrained to matching entries. Non-root panes now
  prepend a non-selectable synthetic `..` row whose Enter action uses the existing parent
  navigation path; POSIX, drive-letter, URI-derived drive, and UNC roots omit it.
- 2026-07-31 codex: Typeahead prefixes now persist behind a full-height footer divider until
  explicitly edited or cleared. Backspace removes one prefix character before falling back to
  parent navigation, Escape clears both typeahead and selection, and an unmatched extension flashes
  red for 400 ms before the typeahead disappears. Four focused pane behavior cases, frontend
  typecheck/build, repository lint, and the full Rust/frontend/script test suite pass.
- 2026-07-31 codex: Typeahead now matches case-insensitive substrings anywhere in full entry names.
  Every visible match highlights only its first occurrence, and constrained keyboard navigation
  uses the same containment rule. Three task-specific reducer/table/pane cases, frontend
  typecheck/build, repository lint, and the full Rust/frontend/script suite pass.
- 2026-07-31 codex: Unmatched typeahead text now remains after its 400 ms red warning so Backspace
  can repair a typo; only Escape explicitly clears it. The focused pane regression test, frontend
  typecheck/build, repository lint, and full Rust/frontend/script test suite pass.
- 2026-08-30: Shift+Arrow while typeahead is active now extends selection within the ordered match
  set rather than converting the next match into a physical row offset. Intervening non-matching
  files can no longer enter the selection, while Shift+Arrow outside typeahead retains its normal
  contiguous-range behavior. The attempted direction is preserved at the first/last match so a
  final Shift+Arrow selects that boundary item without moving the cursor. Space appends to an active
  typeahead prefix; without an active prefix it retains its selection-toggle command.
