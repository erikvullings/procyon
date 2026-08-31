# 0159 Structured viewer Tauri UX regressions

Status: done
Priority: high
Owner: unassigned
Agent: codex
Subsystem: frontend
Depends on: 0100

## Context

Tauri testing of 0100 found that large JSON/JSONL content does not visibly wrap or show useful
syntax colors, its window controls do not follow the existing Materialized control language, CSV
delimiter/header controls render empty and cannot be operated, filtering renders matches in a tiny
secondary strip rather than filtering the table, and sorting is unavailable even for bounded small
CSV files.

## Acceptance Criteria

- Raw JSON/JSONL token spans have visible theme-aware syntax colors and long logical lines wrap in
  the available viewer width.
- JSON window navigation uses supported `mithril-materialized` controls with previous/next icons and
  a clear bounded position indicator.
- CSV delimiter and header choices use interactive controlled Materialized selects and show the
  current values.
- CSV filtering makes the cursor-paged matches the active virtualized table rows; clearing the query
  restores ordinary session rows without opening a secondary results view.
- Fully indexed CSV data below a documented bounded threshold can sort by a selected column in the
  active view. Larger or incomplete sources keep sorting disabled with a specific explanation.
- Focused controller/component tests reproduce each reported Tauri-shared frontend regression and
  pass after the fix.

## Implementation Notes

- Keep all behavior in the shared Mithril frontend; do not add a Tauri-only code path.
- Do not turn bounded sorting into a whole-file backend sort or weaken 0100's memory guarantees.
- Use published `mithril-materialized` component attrs as documented by its installed types.

## Agent Notes

- 2026-08-27 codex: Reproduced from the shared render/controller code. Native selects conflict with
  Materialize styling; search matches are intentionally rendered in a separate strip; JSON uses a
  non-wrapping `pre`; and sorting is unconditionally disabled. Work is isolated in
  `/private/tmp/procyon-structured-viewer-fixes` on `codex/fix-structured-viewer-ux`.
- 2026-08-27 codex: Fixed in the shared viewer/controller. `.jsonl` now uses bounded raw JSON
  windows, mixed keyed/unkeyed token children no longer crash Mithril rendering, token colors are
  theme-aware and long records wrap, and window navigation uses `<`/`>` Materialized FlatButtons.
  CSV delimiter/header controls use controlled Materialized Selects; search matches replace the
  table's active virtualized rows and clearing restores the source page; clickable headers sort
  fully indexed sources up to 1 MiB while larger/incomplete sources retain a precise disabled
  explanation. Focused tests cover rendered controls, interaction, filtering, JSONL routing, and
  bounded sorting. A real-browser computed-style check confirmed wrapping and distinct token colors.
