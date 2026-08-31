# 0114 Decompose Pane Component

Status: done
Priority: medium
Subsystem: frontend
Depends on: none

## Context

The Pane component (`panes/pane.ts`) is 1,324 lines with `PaneAttrs` having ~95 properties. It has absorbed typeahead navigation, inline rename editing, favourites menu management, breadcrumb rendering (5 variants: archive/search/sftp/posix/windows), tab drag-to-reorder, and keyboard dispatch. `WorkspacePaneContent` in `workspace-layout.ts` has ~65 props. Every AppShell closure variable that the Pane touches needs to be threaded through `PaneAttrs`. The current test file is 1,261 lines of brittle DOM-based tests.

## Acceptance Criteria

- **TypeaheadController** — self-contained state machine for typeahead navigation, timers, and character matching
- **BreadcrumbView** module — atomic breadcrumb rendering with all 5 location variants
- **RenameEditingController** — inline edit lifecycle open/save/cancel
- **TabStrip** component — tab rendering with drag-to-reorder
- Pane becomes a composition layer — small, focused, arranging its sub-modules
- `PaneAttrs` reduced to < 40 properties
- All existing Pane tests continue to pass; sub-modules tested without DOM
- Zero change in visible behavior — this is a refactor

## Implementation Notes

- `frontend/src/features/panes/pane.ts` (1,324 lines)
- `frontend/src/features/panes/pane.test.ts` (1,261 lines)
- `frontend/src/features/workspace/workspace-layout.ts` — `WorkspacePaneContent` interface (~65 props)
- Extract sub-modules into `frontend/src/features/panes/typeahead/`, `frontend/src/features/panes/breadcrumbs/`, `frontend/src/features/panes/rename-edit/`, `frontend/src/features/panes/tab-strip/` or flat `.ts` files depending on size
- Reference: architecture review — deepening opportunity #3

## Agent Notes

- 2026-08-10 claude: Extracted TypeaheadController, BreadcrumbView (breadcrumb-view.ts), RenameEditingController, TabStrip from pane.ts. PaneAttrs reduced from ~70 to 39 properties via 5 sub-objects (FavouritesAttrs, TableConfigAttrs, DirectorySummaryAttrs, FilterAttrs, PaneNavigationAttrs). 42 new sub-module tests (TypeaheadController: 14, RenameEditingController: 8, plus 20 passing in prior test files). All 63 existing pane tests pass. TypeScript clean (only pre-existing unrelated errors remain). pane.test.ts attrs() factory accepts flat legacy-style overrides for zero call-site churn. TabStrip manages its own drag state internally.
