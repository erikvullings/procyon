# 0029 Sorting and file metadata summary

Status: done
Priority: medium
Owner: unassigned
Agent: unassigned
Area: frontend
Depends on: 0028

## Context
`file-manager-coding-agent-spec.md` §16 milestone 1 (basic sorting, file metadata summary) and §15
(sorting, multiple sort keys later).

## Acceptance Criteria
- Click or keyboard on a column header sorts ascending/descending by name, extension, size or
  modified time; the active sort is indicated in the header and the status bar.
- Directories sort before files by default, configurable later; the setting is read from settings
  (0030), not hard-coded in the component.
- Sorting is stable, case-insensitive-but-deterministic for names, and uses a natural/numeric
  comparison for names containing digits.
- Sort compares raw values, never formatted display strings (§20 sample plugin 2 makes this a
  general rule).
- Sorting a 100,000-entry directory does not block the UI beyond one frame; measured.
- A metadata summary panel or status area shows details for the cursor entry, fetched lazily through
  `getEntryMetadata` and cancelled when the cursor moves on (§5.2).
- Size and date formatting respect the settings-driven size format and date format (0030).
- Vitest tests cover comparator behaviour (including Unicode, numeric names, missing sizes for
  directories) and lazy metadata cancellation.

## Implementation Notes
- The multi-key sort model can be represented now as a one-element list so §15's "multiple sort keys
  later" needs no rewrite — but do not build the UI for it.
- Server-side sort options exist in the list request (0019); prefer sorting the loaded page in the
  frontend for responsiveness and let the server sort when paging.

## Agent Notes
- 2026-07-31 codex: Added typed one-key frontend sorting for name, extension, raw size and raw
  modified time, with stable natural/numeric Unicode-aware name ordering, deterministic case ties,
  persisted per-tab folder grouping, clickable/keyboard-accessible sort headers, and active sort
  indicators in both the header and status bar. A cooperative stable merge sort yields within an
  8 ms work budget; its 100,000-entry measurement remains below one 16.7 ms frame per slice.
- 2026-07-31 codex: Added settings-driven binary/decimal/byte size and medium/short/ISO date
  formatting seams for task 0030, plus a cursor-driven metadata summary backed by
  `getEntryMetadata`. Cursor moves, clearing, and disposal abort in-flight requests, and late
  responses cannot replace the current entry.
- 2026-07-31 codex: Verified 21 dedicated cases in the three new sorting/formatting/metadata test
  files and 4 task-specific table/pane/AppShell integration cases. Frontend typecheck and production
  build, repository formatting/Clippy/Biome lint, the other 201 frontend tests, and the 3 proxy
  socket tests pass. The full suite remains red only because an unrelated concurrent
  `frontend/src/themes/theme.css` edit removes the cursor outline required by its existing test; it
  was preserved and excluded from this task's commit. No `CLAUDE.md` exists to update.
