# 0067 Quick filter

Status: done
Priority: medium
Owner: unassigned
Agent: unassigned
Area: frontend
Depends on: 0029

## Context
`file-manager-coding-agent-spec.md` §24 item 1 and §16 milestone 3. The quick filter narrows the
currently loaded directory and must stay entirely responsive.

## Acceptance Criteria
- `Ctrl/Cmd+F` opens an inline filter in the active pane; `Esc` clears and closes it.
- Filters the loaded snapshot in the frontend with plain-text matching, case-insensitive, updating
  as the user types with no perceptible lag on 100,000 entries.
- The status bar shows "N of M shown"; clearing restores the full list.
- Cursor and selection behave sensibly across filtering: selection is preserved by `EntryId` and
  hidden-but-selected entries are reported in the status bar.
- Filtering interacts correctly with paging: it is clear whether unloaded pages are excluded, and
  the UI says so rather than implying the directory has fewer entries.
- Glob and regex modes are designed for but not implemented (§24); the mode enum exists with one
  variant.
- Vitest tests: matching, selection preservation, status counts, clear behaviour.

## Implementation Notes
- Distinct from filesystem search (0068) — this never hits the backend.
- Hidden-file visibility is a separate setting, not a filter.

## Agent Notes
- 2026-08-01 copilot: Implemented the inline quick filter end-to-end, entirely frontend, per the
  Implementation Notes (never hits the backend).
  - New feature module `frontend/src/features/quick-filter/`: `quick-filter.ts` (pure functions —
    `matchesQuickFilter`, `filterEntries`, `hiddenSelectedEntryCount`, plus the single-variant
    `QuickFilterMode = 'plainText'` enum required by the acceptance criteria), `quick-filter-input.ts`
    (presentation-only `QuickFilterInput` component: self-focusing `<input>`, `Escape` -> `onClose`,
    `Enter`/`blur` -> `onCommit`, every keystroke -> `onQueryChange`), `quick-filter.css`.
  - `frontend/src/app/app-shell.ts` holds the new per-pane UI-only state: `quickFilterDrafts`
    (`Map<PaneId, string>`, the live uncommitted-per-keystroke text) and `quickFilterOpen`
    (`Map<PaneId, boolean>`, whether the box is shown). `entriesFilteredFor` memoizes
    `filterEntries(sorted, query)` per pane keyed on `(entries reference, query)`, mirroring
    `entriesSortedFor`'s cache-check pattern — no chunked/async treatment needed: the
    `filterEntries` Vitest perf test asserts a single linear pass over 100,000 entries finishes
    well under a frame budget, so `sortEntriesResponsive`-style chunking was unnecessary (measured,
    not assumed).
  - Persistence-timing decision: **on-commit, not per-keystroke.** Per-keystroke input only updates
    the local `quickFilterDrafts` draft (drives filtering/redraw immediately, feels responsive with
    zero backend chatter); the committed query is written to the tab's `view.quickFilter` via
    `dispatchWorkspaceCommand({ type: 'updateView', ..., patch: { quickFilter: ... } })` — the exact
    same mechanism `onSortChange` already uses — only on blur, `Enter`, or when the box is closed
    (if a query was ever persisted, closing clears it). Chosen because per-keystroke workspace
    commands would be needlessly chatty for a feature explicitly framed as backend-free/responsive,
    and blur/Enter/close are the natural "I'm done typing" signals; a dedicated Vitest test
    (`persists the committed quick-filter query and restores it when the filter box reopens`)
    exercises the round trip through a fresh `AppShell` mount to prove it survives remount/reload.
  - Cursor-across-filtering: `cursorIndex` in `paneContent()` is computed via
    `entryIds.indexOf(selection.cursorEntryId)` against the **filtered** entry list (`entryIds` is
    now derived from `filtered`, not the unfiltered `sorted` array), so a hidden cursor entry
    naturally yields `-1`/no visible cursor rather than an out-of-range index into `DirectoryTable`
    — chosen over "jump to first match" as the simpler, always-correct option; nothing in
    `DirectoryTable` needs a valid cursor at all times.
  - Selection preservation: filtering never touches `selectedEntryIds` (still the pane-independent
    `reduceSelection` state) — `hiddenSelectedEntryCount(directory.entries, filtered, selectedEntryIds)`
    counts selected-but-hidden entries for the status bar without pruning anything.
  - Status bar (`frontend/src/features/panes/pane.ts`, `.fm-pane-status`): renders
    `"{ordinaryEntries.length} of {totalEntryCount} shown"` (plus a `" (more available)"` suffix
    when `hasMore === true`, addressing the paging-clarity requirement) when a filter query is
    active, else the original `"{count} entries"` label; the selected-count span gains
    `" ({hiddenSelectedCount} hidden by filter)"` only when `hiddenSelectedCount > 0`.
  - Ctrl/Cmd+F and Esc wiring: **`Ctrl/Cmd+F` goes through the existing generic `core.quickFilter`
    action-dispatch path** in `handleGlobalKeydown` (same `dispatchKeybinding`/`dispatchedAction`
    branch style already used by `core.createDirectory`'s local-dialog-open branch), opening the box
    for the active pane and seeding its draft from any already-persisted query — chosen over a
    bespoke `event.key === 'f'` special case for consistency with every other keybound action and
    because it automatically inherits the existing editable-target guard (`isEditableTarget`
    routes keydown to the `'pathInput'` scope, so `core.quickFilter` never dispatches while another
    input/textarea/contenteditable has focus). **`Esc` is handled locally inside
    `QuickFilterInput`'s own `onkeydown`** (not the global handler) — since Esc only matters while
    the filter input itself has focus, handling it right there is simpler and can't conflict with
    other Escape consumers (command palette, rename-cancel) elsewhere in the shell.
  - Backend: flipped the one line in `crates/fm-application/src/action.rs::core_actions()` —
    `core.quickFilter` now uses `ActionContextRequirements::none()` instead of `::unimplemented()`.
    Confirmed the "features without an implementation" test
    (`features_without_an_implementation_are_registered_as_unavailable`) never actually listed
    `core.quickFilter` (only `core.copyPath`/`core.copyRelativePath`) — no test update was needed
    beyond the one-line capability change.
  - No DTO/OpenAPI changes: `PersistedFilterDto`/`QuickFilterPatchDto`/
    `DirectoryViewConfigurationDto.quickFilter` and the frontend's `TabState`/
    `DirectoryViewConfiguration.quickFilter` field already existed (tasks 0078/0080); confirmed
    `frontend/openapi/openapi.json` and `frontend/src/api/generated/` have zero uncommitted changes.
  - Tests added: `frontend/src/features/quick-filter/quick-filter.test.ts` (5: case-insensitive
    matching, blank-query reference passthrough, name-match filtering, a 100k-entry perf sanity
    check, hidden-selected counting). `frontend/src/features/panes/pane.test.ts` (+4: box
    renders/focuses only when open, keystroke/commit/close callbacks fire correctly, "N of M shown"
    + paging-note + revert-on-clear, hidden-selected status text). `frontend/src/app/app-shell.ts`
    (+3 in `app-shell.test.ts`): Ctrl+F opens/filters/Escape closes end to end, repeated Ctrl+F /
    editable-target-focused is a no-op, and committed-query persistence survives a remount. Plus
    `workspace-layout.test.ts` fixture additions (no new `it`s) for the new `WorkspacePaneContent`
    fields. Total: 46 test files / 317 tests pass (up from 305 before this task, +12 new).
  - Verification commands run: `cargo test -p fm-application` (113 unit + all integration incl.
    `conflict_resolution` passing this run), `cargo clippy -p fm-application --all-targets -- -D
    warnings` (clean), `cargo fmt -p fm-application --check` (clean); `npx tsc --noEmit -p
    tsconfig.json` (clean); `pnpm run lint:frontend` (biome — clean except 2 pre-existing
    `noImportantStyles` warnings in `pane.css`, unrelated to this task, not introduced by it);
    `pnpm --dir frontend test -- --run` (46 files / 317 tests, all pass).
  - Left `frontend/src/api/client/tauri-file-manager-client.ts`'s pre-existing unrelated 1-line
    change (`import type { FileManagerClient }`) untouched and unstaged, per instructions.
