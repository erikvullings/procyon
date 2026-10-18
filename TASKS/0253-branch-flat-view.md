# 0253 Branch (flat) view

Status: open
Priority: medium
Subsystem: backend, frontend
Depends on: 0019, 0024

## Context
Total Commander's Ctrl+B "branch view" lists every file under the current folder (recursively) as one flat list, so users can sort/filter/select across subfolders (e.g. largest files, all `*.log`) and run normal operations on the result. Not present today (no recursive listing mode in the directory service; search is separate).

## Acceptance Criteria
- Toggle action (default Ctrl+B, command palette, view menu) switches the active pane tab to a flat recursive listing of the current location; toggling again restores the normal listing.
- Entries show their relative parent path (column or tooltip); sorting, quick filter, selection, and file operations (copy/move/delete/rename, basket) work on flat entries by full location.
- Streams in pages with cancellation; virtualized; never follows symlinks; stops at provider boundaries (archives, remote) unless the provider supports it cheaply.
- Navigating, refreshing, or closing the tab cancels the scan; watch deltas keep the list correct or mark it stale.
- Works in mock, HTTP, and Tauri adapters (spec rule 9).

## Implementation Notes
- Prefer a backend recursive-listing mode in the directory service (reuse the walker from `fm-search`) exposed as a virtual-folder-like source, similar to Basket virtual folder (0238) and smart folders (0162).
- Cap/limit with explicit "truncated" state rather than unbounded memory.

## Agent Notes
- Created 2026-10-18 from the feature gap review. No work started.
