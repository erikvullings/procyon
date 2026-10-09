# 0240 Background activity and quit safety

Status: done
Priority: high
Subsystem: frontend
Depends on: 0036, 0047, 0182

## Context
Large permanent deletions may remove many items before the directory listing changes. The
existing toolbar progress mark covers file operations only and is easy to miss. Semantic indexing
is background work too. Desktop quit currently gives no warning that file operations will not
resume automatically, while managed semantic indexing reconciles enrolled roots on restart.

## Acceptance Criteria
- A compact, accessible activity indication remains visible while file operations or semantic
  indexing run, with a count and useful progress text. Show percentages only for known totals;
  navigate directly to the relevant operation or semantic status from the activity display.
- Quit with active work warns that file operations can leave partial results and are interrupted
  rather than resumed. Continue and stay are explicit choices; all desktop quit paths are covered.
- A source folder in an active move or deletion has a restrained in-progress state in directory
  listings, without skipping it during keyboard navigation. Clear the state on terminal outcome
  and refresh the listing from the backend.
- Cover active, terminal, unknown-total, and interrupted states with focused tests. Keep browser
  and Tauri behavior consistent where the host supports the feature.

## Implementation Notes
- Reuse `OperationCentreState` and backend semantic ingestion events, not a second operations
  store. Source identity must be compared by location, not display name; never imply a delete has
  completed until the backend confirms it.
- The desktop native menu can quit without calling the frontend client. Intercept app exit at the
  native lifecycle boundary, not only at the keyboard handler.

## Agent Notes
- 2026-10-09 Copilot: Started after confirming the worktree had no uncommitted changes.
- 2026-10-09 Copilot: Added an activity badge and per-job navigation, semantic reconciliation
  progress, navigable source-folder states, and native desktop quit confirmation. Focused
  frontend and Rust tests, all frontend tests, workspace lint, and Rust doctests pass.
  The full Rust run had one unrelated intermittent Docling descendant-cleanup failure
  (2809/2810 tests passed); the native-SPA smoke test timed out both in the script
  suite and in isolation. Native quit behavior was compiled but not manually exercised.
