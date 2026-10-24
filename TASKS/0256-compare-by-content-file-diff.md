# 0256 Compare by content and file diff

Status: done
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
- 2026-10-24 Claude: Implemented.
  - **Verified 0075 first**: only `nameOnly`/`sizeAndTimestamp`/`contentHash` (SHA-256 of whole files) existed, no early-exit byte
    compare and no UI to pick criteria.
  - **Backend**: `fm_comparison::compare_streams` (new `content.rs`; chunked, cancellable, first differing offset) backs a new
    `ComparisonCriteria::Content` ("by content"): the engine skips reading files of known different size, streams equal-size pairs
    until the first difference, classifies via `classify_with_content` (identical bytes win over timestamps; differing bytes keep the
    newer/older direction; no verdict falls back to size+timestamp, counted as a warning). Runs inside the existing cancellable
    comparison job/operation-centre entry. New `FileDiffService` (`fm-application/src/file_diff.rs`, `similar` Myers with a 5 s deadline,
    intra-line char diff honouring ignore options, text rows + `changeStarts`; binary => stream compare + first offset, also for files
    over the cap; text over 2 MiB or 100 000 lines per side => `tooLarge`). `FileManagerService::diff_files` is a thin delegate. DTOs in
    `fm-transport-dto/src/file_diff.rs`, `POST /api/v1/file-diff` (`diffFiles`, accessible-roots guarded) and Tauri `diff_files`;
    OpenAPI + Orval regenerated.
  - **Frontend**: `diffFiles` on mock/HTTP/Tauri clients (mock has its own small LCS diff, `mock-file-diff.ts`); `FileDiffDialog`
    (side-by-side, virtualized via `calculateVisibleWindow`, intra-line marks, next/previous change with wrap, ignore whitespace/case
    toggles, binary and too-large notices); actions `client.compareFiles` (two selected files in the active pane or one per pane,
    context menu + registry + availability) and `client.compareByContent` (compare panes with the `content` criterion); i18n in all 8 locales.
  - **Verified**: `fm-comparison` 8 new tests (`content::tests` 5, `model` 2, engine integration 1; 50 total pass); `fm-application --lib
    file_diff` 13; `fm-server --test files_routes diff` 1; frontend `file-diff-actions` 5, `file-diff-dialog` 7, `mock-file-diff` 4,
    availability +3, mock client +2, http client +1; `pnpm run lint` clean; full `cargo nextest` for comparison/dto/application/server/
    test-support (1586) and `vitest run` (3113) pass; `tsc --noEmit` clean.
  - **Known gaps**: no dedicated shortcut for Compare files (command palette / context menu only); no in-dialog UI to pick comparison
    criteria (the by-content comparison is its own action); the diff is request/response (cancelled when the request is dropped), not an
    operation-centre job; no 403 REST test (the test server has no accessible-roots restriction; the guard is the shared helper used by
    other routes); merge/apply-hunk remains the follow-up; the display is a custom virtualized view, not CodeMirror merge.

