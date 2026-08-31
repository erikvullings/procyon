# 0056 Sample plugin: File Age column

Status: done
Priority: medium
Owner: unassigned
Agent: unassigned
Area: plugins
Depends on: 0054, 0024

## Context
`file-manager-coding-agent-spec.md` §20 sample plugin 2 and §36 item 9. Demonstrates custom column
registration and the separation of sort value from display value.

## Acceptance Criteria
- `plugins/sample-file-age-column/` registers the column `sample.fileAge`.
- Displays a compact age derived from the modification time: `5m`, `3h`, `2d`, `4mo`, `2y`.
- Sorting uses the raw timestamp, never the formatted text (§20).
- The column refreshes on a coarse interval (e.g. once a minute) without redrawing every second and
  without re-rendering rows that did not change (§20, §28).
- The column can be added/removed via column configuration and its state persists in settings.
- Rendering a 100,000-entry directory with the column enabled does not measurably degrade scrolling;
  measured against the 0024 benchmark.
- If the plugin errors or times out, the cell renders empty and the table keeps working (§19.4).
- Tests: age formatting boundaries (59s/1m, 23h/1d, 30d/1mo, 12mo/1y), sort-by-raw-value,
  refresh interval behaviour.

## Implementation Notes
- This is the first consumer of the directory table's plugin-column seam (0024) — expect to finish
  that seam here.
- Column values must be computed from already-available metadata; no extra filesystem calls per row.

## Agent Notes
- 2026-08-01 Codex: Added the bundled `sample-file-age-column` Lua declaration and a typed,
  data-only plugin-column transport path. The host renders compact age values from loaded metadata,
  sorts `sample.fileAge` by raw modification timestamp, and refreshes at a one-minute cadence.
  Visibility follows the persisted workspace column configuration. Plugin declaration errors are
  isolated and omit their cells without interrupting the table. Verified targeted frontend tests
  (30), plugin/runtime/application Rust tests, strict frontend typechecking, the directory-table
  benchmark with the column enabled, and the full `pnpm test` suite. Repository lint reports only
  three existing Biome warnings in unrelated files.
