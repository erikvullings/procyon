# 0116 Centralize Selections-to-Locations Translation

Status: done
Priority: medium
Subsystem: frontend
Depends on: none

## Context
The translation from `SelectionState` → array of location URIs (needed for copy, move, paste, delete, etc.) is duplicated in approximately 15 places across AppShell. The selection module (`selection/selection.ts`) is a deep state machine, but nothing downstream leverages that depth. Every caller re-derives "what entries are selected in this pane?" from the selection state, instead of calling a single method.

## Acceptance Criteria
- `getSelectedEntryUris(selection, directoryEntries)` added to selection module interface
- Optionally `getSelectedEntries(selection, directoryEntries)` for callers needing full Entry objects
- All ~15 AppShell call sites replaced with the single function
- Function is pure, tested with selection states from existing `selection.test.ts`
- Zero change in visible behavior — this is a refactor

## Implementation Notes
- `frontend/src/features/selection/selection.ts` (143 lines) — add functions here
- `frontend/src/app/app-shell.ts` — ~15 scatter sites to replace
- `frontend/src/models/location.ts` — location URI types
- Lowest-effort, highest-leverage quick win
- Reference: architecture review — deepening opportunity #5

## Agent Notes
- 2026-08-10 Erik/Vullings: Added `getSelectedEntries(selection, entries)` and `getSelectedEntryLocations(selection, entries)` to `selection.ts`. Both are pure functions that filter directory entries by selection state using a Set for O(1) lookup. Replaced all 11 occurrences of inline `selection?.selectedEntryIds.includes(entry.id)` filtering in AppShell (~11 call sites across clipboard, commands, copy, move, pack, moveToArchive, trash, delete, view, edit). Also replaced 1 occurrence of `context.selectedEntryIds.includes` with a `Set`-based filter. Added 10 new tests covering undefined selection, empty selection, single selection, discontinuous multi-selection, no-overlap, empty entries, order preservation, and location equivalence. AppShell shrank by ~45 lines net. All 20 selection tests pass. No new type errors. No new test failures introduced (4 pre-existing failures in theme/runtests/body/content-search/unrelated files).
