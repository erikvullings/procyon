# 0024 Virtualized directory table component

Status: done
Priority: high
Owner: unassigned
Agent: codex
Area: frontend
Depends on: 0021, 0022, 0013

## Context
`file-manager-coding-agent-spec.md` §15 — the directory table is the critical custom component.
§35 forbids rendering large directories without virtualization. Build it against the mock client
(0013) so it can be developed and benchmarked before the real backend is wired up.

## Acceptance Criteria
- Custom Mithril component (not `mithril-materialized`, not card-based — §14) rendering only the
  visible window of rows plus a small overscan.
- Fixed row height for this version, read from `--fm-row-height`; configurable row height is later.
- Initial columns: name, extension/type, size, modified time.
- States: loading placeholders, empty, error, plus hidden-file styling and symlink/junction
  indicators.
- Renders 1,000 / 10,000 / 100,000 real entries and a 1,000,000-entry mocked dataset without
  mounting every row; a test asserts DOM node count stays bounded while scrolling.
- Scroll and keyboard cursor movement stay responsive; measured with a rendering benchmark
  committed under `frontend/src/features/directory-table/`.
- Accessibility (§29): correct grid/row/cell semantics, visible focus, accessible labels, and the
  focused row is announced; no colour-only status indicators.
- Rows are keyed by stable `EntryId` so deltas patch rows instead of re-creating them.
- Vitest tests cover windowing maths, scroll-to-index, and rendering of each state.

## Implementation Notes
- Selection, cursor and sorting behaviour are separate tasks (0028, 0029); this task provides the
  rendering surface and the cursor/selection *rendering* hooks.
- Design for later: resizable/reorderable/configurable columns, plugin columns, inline rename, drag
  source and drop target — leave the seams without implementing them (§15, §35).

## Agent Notes
- 2026-07-30 codex: Implemented a custom fixed-row Mithril grid backed by a random-access entry
  source, with bounded visible-window rendering and overscan, stable `EntryId` keys, initial
  name/type/size/modified columns, loading/empty/error states, hidden and link/junction badges, and
  cursor/selection rendering hooks. Added semantic grid/row/cell metadata, keyboard-visible focus,
  active-row announcement, and non-colour status indicators. The column descriptor boundary,
  random-access source, and rendering-only cursor/selection inputs leave the planned extension
  points without implementing tasks 0028/0029.
- 2026-07-30 codex: Added and explicitly verified 15 task-specific Vitest tests across
  `windowing.test.ts`, `directory-table.test.ts`, and `directory-table.benchmark.test.ts`. Coverage
  includes windowing maths, scroll-to-index, every state, stable keyed patching, semantic/status
  rendering, 1,000/10,000/100,000 materialized entries, and bounded DOM scrolling/cursor redraws
  against a lazy 1,000,000-entry source. Strict frontend typecheck, production build, all 110
  frontend tests, the complete repository test command, Rust fmt/clippy, and scoped Biome checks
  pass. Real Chrome verification mounted 17–20 rows while scrolling the million-entry harness
  halfway and kept the header sticky.
  Repository-wide `pnpm run lint` remains blocked only by the pre-existing Biome formatting
  failures in `scripts/architecture-docs.test.mjs` and `scripts/ci-workflow.test.mjs`; no Tauri
  runtime UI smoke test was added because this host-neutral component has no transport integration.
- 2026-07-31 codex: Follow-up wired the table into the mounted application shell. Mock runtime now
  loads `mock:///` through `FileManagerClient` with cancellation and displays the resulting entries;
  other runtimes show the table's idle state until workspace navigation is implemented. Replaced the
  direct Mergerino store wiring with `meiosis-setup`, retaining frame-batched publications, targeted
  subscriptions, and one redraw per frame.
- 2026-08-30: Fixed paged directories briefly exposing blank virtual rows before requesting the next
  page. The table now prefetches one viewport before the loaded boundary. Reproduced against the real
  535-entry Downloads directory through the Axum/local-filesystem path and retained the existing
  background full-directory type-to-select behavior.
- 2026-08-30: Reduced the Ext column's default track from 6rem to 5rem and gave it a 48px
  column-specific resize floor instead of the shared 60px minimum.
- 2026-08-30: When Ext is visible, file rows show the filename stem in Name and the extension only
  in Ext. Hiding Ext restores the full filename in Name; directories, symlinks, extensionless files,
  and leading-dot files always retain their complete names.
