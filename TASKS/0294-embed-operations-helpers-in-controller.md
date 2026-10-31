# 0294 Operations confirmation/guard decorators

Status: dropped
Priority: low
Subsystem: frontend
Depends on: 0290

## Decision

Not worth doing as originally written. `withOperationConfirmation`, `withPermanentDeleteConfirmation`
and `withActiveSourceGuard` are decorators that wrap an `OperationsController`; moving them inside
the controller would make it depend on UI confirmation callbacks. They have a single production call
site (the `ops.create` block in `app-shell.ts`), so there is no scattered usage to consolidate.

If that call site is still awkward after task 0290, fold a `createGuardedOperationsController(client,
ui)` factory into 0290's operations wiring instead of opening a separate task.
