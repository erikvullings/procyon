# 0112 Extract Operations Controller from AppShell

Status: done
Priority: high
Subsystem: frontend
Depends on: none

## Context

All operation initiation (copy, move, trash, delete, pack, extract, paste) is inline keyboard-handling code inside `app-shell.ts` closure. Every call is a raw `client.startOperation(...)` with hardcoded `conflictPolicy: 'ask'`. A duplicate "selections → locations" lookup appears in ~15 places. The `operation-state.ts` reducer manages UI state but has no role in initiating operations. Adding a new operation type means editing among 100+ lines of dispatch logic in AppShell.

## Acceptance Criteria

- `OperationsController` module with small interface (`copy`, `move`, `trash`, `paste`, `pack`, `extract`, etc.)
- All operation initiation in AppShell replaces inline `startOperation` calls with controller methods
- Selections→locations translation centralized inside the controller
- Conflict policy defaults and conflict dialog orchestration encapsulated
- Zero change in visible behavior — this is a refactor
- Tests for OperationsController using `MockFileManagerClient`

## Implementation Notes

- `frontend/src/app/app-shell.ts` — scattered operation dispatch (~200 lines)
- `frontend/src/features/operations/operation-state.ts` — pure reducer (leave as UI state seam)
- New module: `frontend/src/features/operations/operations-controller.ts`
- Controller depends on `FileManagerClient` and the clipboard/selection modules
- Reference: architecture review — deepening opportunity #1

## Agent Notes

-
- 2026-08-10 claude: Created OperationsController module at frontend/src/features/operations/operations-controller.ts. All startOperation calls in AppShell keyboard handlers and context menu handlers replaced with controller methods. 11 tests in operations-controller.test.ts. TypeScript clean, all tests pass.
