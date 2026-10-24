# 0254 Branch (flat) view

Status: done
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
- 2026-10-24 claude: Implemented as a frontend-only composition of the existing search virtual
  location: `core.branchView` (Cmd/Ctrl+B, command palette) runs a recursive, files-only search with
  an empty name filter rooted at the active folder in the same pane
  (`FindFilesController.toggleBranchView`); invoking it again from that result cancels the scan and
  navigates back to the stored root. Search tabs already provide parent-path rendering, sorting,
  quick filter, selection, operations on real locations, paging, cancellation (also on
  navigate/close), symlink non-following and provider-boundary handling, in mock/HTTP/Tauri alike.
  Verified: 3 new controller tests + 1 palette test, tsc clean, full frontend suite (3038) and
  `fm-application` action tests pass.
- Known gaps: result lists are not live-updated from watch deltas (same as every search tab; the
  existing refresh control reruns it); the backend search result cap/truncation behaviour is the
  existing search one, no branch-specific cap. No native menu-bar item (palette + shortcut only).
