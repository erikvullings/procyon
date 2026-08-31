# 0026 Two-pane workspace layout and pane focus

Status: done
Priority: high
Owner: unassigned
Agent: unassigned
Area: frontend
Depends on: 0025

## Context
`file-manager-coding-agent-spec.md` §14 (main window) and §36 item 3. The engine must not hard-code
exactly two panes (§5.3), but the first UI shows two.

## Acceptance Criteria
- Main window layout matches §14: application bar, workspace toolbar row, left/right panes,
  operation centre area (placeholder until 0036), optional function-key bar.
- Two panes side by side with a draggable splitter; the split ratio persists via the workspace's
  `UpdateLayout` command (0080), debounced per §5.3.8 — not via 0030, which no longer owns live
  workspace layout state.
- Exactly one pane is active; `Tab` switches panes and focus follows, with visible focus (§29).
- Clicking anywhere in a pane makes it active.
- The layout is driven by `WorkspaceLayout` from the backend workspace model, so a future
  three-pane layout needs no component rewrite.
- Window resize keeps both panes usable down to a reasonable minimum width.
- Vitest tests cover: pane activation, `Tab` switching, splitter constraints.

## Implementation Notes
- The function-key/action bar can render placeholder labels (F5 Copy, F6 Move, ...) that become live
  once the action registry lands (0050).
- Operation centre area is a stub that 0036 fills in.

## Agent Notes
- 2026-07-31 codex: Added a recursive Mithril workspace renderer driven by the backend
  `WorkspaceLayout` tree, including horizontal and vertical splits, minimum-size ratio clamping,
  immediate splitter feedback, and 500 ms debounced semantic `UpdateLayout` persistence. Pane
  clicks activate the containing pane; Tab/Shift+Tab traverse leaves in layout order and transfer
  DOM focus with visible, non-colour-only focus styling. A nested three-pane fixture verifies that
  no two-pane-specific component rewrite is required.
- 2026-07-31 codex: Reworked the application shell to load/create and open its workspace through
  the transport-neutral client, load each pane's active directory, and render the §14 application
  bar, workspace toolbar, recursive pane area, operation-centre placeholder, and function-key bar.
  Added 6 task-specific Vitest tests (5 in `workspace-layout.test.ts`, 1 new shell-composition test)
  covering activation, Tab focus, recursive order, splitter constraints, debounced persistence,
  and main-window regions. Frontend typecheck, all 127 frontend tests, production build, repository
  lint, and the complete Rust/frontend/script test suite pass. The shared Tauri host startup test
  passes; Windows visual behaviour was not manually exercised on this macOS development host.
