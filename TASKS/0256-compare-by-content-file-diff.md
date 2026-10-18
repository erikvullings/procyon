# 0256 Compare by content and file diff

Status: open
Priority: medium
Subsystem: backend, frontend
Depends on: 0075, 0099

## Context
Directory comparison (0075) exists, but there is no byte/content comparison mode for equal-size files (confirm in 0075 notes) and no text diff view for two files (TC "Compare by content", Ctrl+F3-style diff). Needed to answer "what actually changed".

## Acceptance Criteria
- Directory comparison offers a "by content" criterion (streamed, cancellable byte compare; optionally checksum-based via 0077), as a job through the operation engine, not TypeScript.
- Action "Compare files" with exactly two selected files (or one per pane) opens a side-by-side/inline text diff with change navigation, intra-line highlights, and ignore-whitespace/case toggles.
- Binary files: report identical/different with first differing offset; no text diff rendered.
- Size caps with a clear message for huge files; virtualized rendering.
- Read-only first; merge/apply-hunk is a follow-up task.

## Implementation Notes
- Diff algorithm in Rust (e.g. `similar` crate) behind an application service (new capability module, not in `FileManagerService`), DTOs in `fm-transport-dto`, OpenAPI regenerated via `pnpm api:export`/`api:generate`.
- Reuse the viewer/editor shell (CodeMirror merge view is an option) for display.

## Agent Notes
- Created 2026-10-18 from the feature gap review. Verify what 0075 already compares before starting.
