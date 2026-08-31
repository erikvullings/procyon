# 0025 Pane component: tab strip, breadcrumb path bar and status bar

Status: done
Priority: high
Owner: unassigned
Agent: unassigned
Area: frontend
Depends on: 0024

## Context
`file-manager-coding-agent-spec.md` §14 (main window layout) and §33 step 5. A pane composes a tab
bar, a breadcrumb/path input, the directory table and a status bar.

## Acceptance Criteria
- `features/panes/` provides a `Pane` component containing: tab strip (single tab for now),
  breadcrumb path bar, directory table, status bar.
- The breadcrumb shows each path segment as a clickable target and switches to an editable text
  input on click or `Ctrl/Cmd+L`, with `Esc` to cancel and `Enter` to navigate.
- Path input accepts absolute paths, `~`, and paths with spaces; invalid paths show an inline error
  without clearing the current view.
- Status bar shows: entry count, selected count, selected size, and the current sort.
- The active pane is visually distinct; the inactive pane shows dimmed selection
  (`--fm-selection-inactive`).
- Compact, information-dense layout per §14 "visual direction"; no card-heavy styling.
- Vitest tests cover breadcrumb segment generation (including root and UNC cases), edit-mode
  toggling, and status bar counters.

## Implementation Notes
- The pane holds presentation state only; all filesystem state comes from the backend (§3 rule 8).
- Tab strip renders one tab now; multi-tab behaviour is task 0069.

## Agent Notes
- 2026-07-31 codex: Added a presentation-only Mithril `Pane` composing the task 0024 virtualized
  directory table with a compact single-tab strip, cumulative clickable POSIX/home/drive/UNC
  breadcrumbs, Ctrl/Cmd+L and click-to-edit path input, Escape/Enter handling, inline validation
  and backend navigation errors, and entry/selection/selected-size/sort status counters. Wired the
  mock application shell through the pane without moving filesystem state into the component;
  active panes use the existing active selection token while inactive panes retain
  `--fm-selection-inactive`.
- 2026-07-31 codex: Added and explicitly verified 10 task-specific Vitest tests in `pane.test.ts`
  plus the app-shell composition assertion. Frontend typecheck, all 121 frontend tests, production
  build, repository lint (Biome, Rust fmt and clippy), and a real-Chrome render smoke check pass.
  The complete repository test command reached an unrelated Rust property-test failure because its
  generator produced the reserved Windows name `AuX.`; all affected-package tests pass and no
  regression seed file was retained.
