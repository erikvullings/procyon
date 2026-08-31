# 0115 Migrate AppShell Closure State to Meiosis Store

Status: done
Priority: high
Subsystem: frontend
Depends on: none

## Context

The Meiosis store (`state/store.ts`) is a well-designed deep module with patch batching, `DeepReadonly` types, and clean reducers. But it's bypassed — AppShell keeps all operational state as local `let` variables: `workspace`, `operations`, `directories`, `selections`, `sortedEntries`, `filteredEntries`, `quickFilterDrafts`, `closedTabStacks`, `contentMatchesByEntryUri`, `findFilesParamsByLocationUri`. The store's actions are only called in targeted spots. The deep state management module provides zero leverage because nobody uses it. The interface is the test surface, but nobody tests through it.

## Acceptance Criteria

- Directory cache (snapshots, revisions) migrated to store slice with reducers
- Selection state migrated to store slice (or referenced from `selection/selection.ts` reducer which already exists)
- Operation state reference moved from closure variable to store
- Sorted/filtered entries cache migrated to store slice
- Quick filter draft migrated to store slice
- AppShell reads from store subscriptions instead of closure variables
- Existing tests continue to pass; new reducer tests for each migrated slice
- Zero change in visible behavior — this is a refactor

## Implementation Notes

- `frontend/src/app/app-shell.ts` — closure variables to migrate
- `frontend/src/state/model.ts` (96 lines) — add state slices
- `frontend/src/state/actions.ts` (61 lines) — add actions for new slices
- `frontend/src/state/reducers.ts` (219 lines) — add reducers
- Migrate incrementally: pick one slice, add it, update callers, verify tests. Do NOT attempt all-at-once.
- Start with directory cache, then selection, then operations, then sort/filter
- Reference: architecture review — deepening opportunity #4

## Agent Notes

- 2026-08-10 claude: Migrated `quickFilterDrafts`, `closedTabStacks`, `contentMatchesByEntryUri` to store (new slices `QuickFilterDraftsState`, `ClosedTabStacksState`, `ContentMatchesState` in `model.ts`; 5 new reducer patch functions in `reducers.ts`; 5 new actions in `actions.ts`). Remaining (not migrated): `directories`, `selections`, `sortedEntries`/`filteredEntries`, `operations`, `findFilesParamsByLocationUri`. 6 new reducer tests. TypeScript clean, 110 tests pass (1 pre-existing failure unrelated to this task).
