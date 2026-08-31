# 0100 Read-only streaming structured-data viewer

Status: done
Priority: high
Owner: unassigned
Agent: codex
Subsystem: frontend, backend
Depends on: 0088

## Context

Procyon's F3 Lister viewer can open text-like files through bounded range requests, but it does not
yet provide a structured view for very large CSV, TSV, JSON, NDJSON, or spreadsheet files. Users
should be able to inspect multi-gigabyte data without loading the full source into frontend or
backend memory. CSV headers should remain visible, JSON should remain syntax-highlighted, and
search/filtering should work outside the currently visible window.

This task is deliberately **read-only**. Large structured-file editing is separated into 0158 so
that an attractive viewer cannot accidentally expose an inadequately proven write path.

The current large-text implementation also needs correction before it can be reused here:
`frontend/src/features/preview/file-viewer-controller.ts::loadMore` appends every 64 KiB chunk to
one growing string. CodeMirror virtualizes DOM rendering, not storage of the supplied document, so
scrolling far enough can still accumulate the entire file in frontend memory.

## Acceptance Criteria

- The F3 viewer uses a genuinely bounded sliding window and small LRU cache. Loaded text, parsed
  records, token spans, and rendered DOM remain within documented memory budgets regardless of
  source-file size; scrolling to EOF must not accumulate the whole file.
- Structured viewing is implemented in `fm-application` behind a provider-neutral session API and
  added to `FileManagerClient`. HTTP and Tauri remain thin, behaviorally equivalent adapters; no
  filesystem path or Tauri-only implementation becomes the application contract.
- A session records a source revision/version and is invalidated with a clear message if the file
  changes while it is being indexed or viewed. Sessions are cancellable and release indexes,
  cached ranges, and temporary resources when closed.
- CSV/TSV dialect detection covers at least comma, semicolon, tab, and pipe delimiters and handles
  UTF-8 BOM input. Detection is presented as a choice the user can correct rather than as an
  infallible conclusion.
- CSV rows are parsed as logical records, including delimiters and newlines inside quoted fields.
  Do not index raw `\n` bytes as row boundaries.
- The CSV index is incremental and sparse: initial rows open before a whole-file scan finishes,
  background indexing reports progress, and memory is not proportional to the number of records.
  Exact total-row count may remain unknown until indexing reaches EOF.
- The CSV table virtualizes rows and wide column sets. A detected first-row header remains sticky,
  and the user can switch between "first row is header" and "no header" without reopening the
  file.
- Large-file sorting is disabled with an explanation. Backend search/filter scans incrementally,
  returns cursor-paged results rather than every matching row number, and can be cancelled.
- Raw JSON syntax highlighting works for multi-gigabyte and single-line/minified JSON without a
  whole-file string or AST. A chunk-safe lexer uses sparse state checkpoints and returns only the
  visible UTF-8-aligned window and token spans.
- Structured JSON rows are offered only where records have safe boundaries: NDJSON and, if the
  implementation can index it without a full AST, top-level arrays. Arbitrary deeply nested JSON
  remains available in raw highlighted mode rather than pretending to be cheaply random-access.
- Excel formats (`.xlsx`, `.xlsb`, `.xls`) are read-only and honestly bounded. Sheet tabs and
  formatted cells may use `calamine`, but the UI must apply explicit workbook/sheet limits and fall
  back to the external application if the parser materializes more data than the viewer budget.
  Do not claim that `calamine::worksheet_range` is a streaming row-range API.
- Providers with `RANDOM_ACCESS` support arbitrary indexed jumps. Sequential-only providers use a
  documented progressive/spooling mode or expose an explicit limitation; they must not silently
  re-read gigabytes from byte zero for each scroll operation.
- Tests cover quoted CSV newlines, BOM/dialect/header overrides, malformed/flexible records,
  minified and multi-byte JSON across chunk boundaries, cancellation, source revision changes,
  sequential-provider degradation, browser/Tauri parity, and a generated large fixture proving
  the memory budget does not grow with source size.
- No edit, insert, delete, save, or source-replacement control is exposed by this task.

## Implementation Notes

- Reuse `fm-vfs`'s `open_read` and capability-gated `read_range`, plus
  `crates/fm-application/src/content_streaming.rs`, rather than accepting native paths.
- Prefer a session contract shaped around `open`, `read window/rows`, cursor-paged `search`, and
  `close`; do not return an unbounded `Vec<u64>` row index over transport.
- The Rust `csv` crate exposes parser positions that can be used as safe record checkpoints. A
  checkpoint every configurable number of records plus a hot-row cache is preferable to one offset
  per row.
- `memmap2`/`memchr` may be a measured optimization inside the local provider path, not a required
  dependency of the provider-neutral contract. A mapped file must be invalidated safely when the
  source changes or is truncated.
- For JSON, checkpoints need enough lexical state to resume correctly (string/escape state,
  container context, nesting depth, and line information). The large mode should use a virtual
  text renderer rather than passing an ever-growing document to CodeMirror.
- Pretty-print only bounded visible values in v1. A whole-file derived pretty document makes source
  offsets, search hits, and memory behavior substantially harder.
- Reuse the directory table's virtualization approach unless a new dependency demonstrates a clear
  advantage in a small prototype.

## Agent Notes

- Initial task setup. No execution attempts recorded yet.
- Key constraint: Do not attempt full-file sorting on streamed files - keep memory footprint
  minimal.
- 2026-08-26: Rescoped after product discussion from a Tauri-specific CSV/Excel design into a
  provider-neutral, read-only structured-data viewer. Added CSV record-boundary correctness,
  bounded JSON lexical highlighting, realistic Excel limits, and the prerequisite fix for the
  existing append-only text window. All large-file writes were split into 0158 because corruption
  risk must not be coupled to shipping the viewer.
- 2026-08-26 codex: Implementation started in isolated worktree
  `/private/tmp/procyon-task-0100` on branch `codex/task-0100-structured-viewer`. Full-workspace
  builds are intentionally avoided; verification will target affected crates/packages first.
- 2026-08-26 codex: Implemented provider-neutral, cancellable structured-view sessions with sparse
  CSV/NDJSON and JSON lexical indexes, source-revision validation, bounded row/byte pages, and a
  four-page hot cache. Added thin HTTP/Tauri adapters, generated contracts, browser/mock parity,
  row-and-column virtualization with a sticky header, bounded raw JSON highlighting, cursor-paged
  search, explicit sorting/sequential-provider limitations, and an honest external-app fallback for
  spreadsheet formats. Existing large-text navigation now replaces its 64 KiB window instead of
  appending indefinitely. Editing remains excluded and tracked in 0158.
- 2026-08-26 codex: Verified with frontend TypeScript, 116 scoped frontend tests plus the focused
  virtualization test, targeted DTO/application/server tests (including quoted newlines, dialect
  override, UTF-8 JSON boundaries, revision/cleanup, sequential degradation, and generated
  100,000-row boundedness), clippy for `fm-transport-dto`, `fm-application`, and `fm-server`, and a
  scoped `fm-desktop --lib` check. OpenAPI export/generation completed without drift in generated
  output; the repository-wide `api:check` final `git diff --exit-code` is expected to remain nonzero
  while this task's changes are uncommitted. The full workspace suite was intentionally not run.
