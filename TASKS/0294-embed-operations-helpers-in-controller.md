# 0294 Move operations confirmation/guard helpers inside the controller

Status: open
Priority: medium
Subsystem: frontend
Depends on: none

## Context

`frontend/src/features/operations/operations-controller.ts` defines the `OperationsController` interface (20+ methods) and three standalone helper functions (`withOperationConfirmation`, `withPermanentDeleteConfirmation`, `withActiveSourceGuard`) that wrap the controller. These helpers are not part of the controller interface but are needed to safely use it — callers must know about them separately. This creates a leaky seam between the controller and the helpers, where the safe usage pattern is scattered across the module rather than encapsulated.

## Acceptance Criteria

- `withOperationConfirmation`, `withPermanentDeleteConfirmation`, and `withActiveSourceGuard` are moved inside the `OperationsController` implementation (e.g. `createOperationsController`)
- The controller interface remains the same (no API breakage for callers)
- Callers use confirmation/guard functionality through the controller interface or the factory function
- The standalone helper functions are removed as exports (except for test imports)
- `pnpm test` passes for all operations-related tests

## Implementation Notes

- The helpers should become private functions inside `operations-controller.ts` or be integrated into `createOperationsController` as closures
- If the helpers need to be called from other modules, expose them as methods on the `OperationsController` interface or as named exports from the factory
- The `execute()` function in the controller factory should handle the confirmation check internally when applicable
- This is a refactor with no behaviour change — all existing callers should continue to work

## Agent Notes

- Check which modules import the three helper functions: `import { withOperationConfirmation, withPermanentDeleteConfirmation, withActiveSourceGuard } from '../operations/operations-controller'`
- The main callers are likely in other feature files that perform operations (copy, move, trash, etc.)
- After the refactor, those callers should call the controller method directly without needing to know about the confirmation wrapper
- The `operation-confirmation-dialog.ts` (180 lines) and `multi-rename-dialog.ts` (481 lines) may reference these helpers
- Keep test coverage intact — the confirmation logic is critical and should be well-tested
