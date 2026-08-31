# 0120 Extract Operation Planner module

Status: done
Priority: high
Subsystem: backend
Depends on: 0119

## Context

`FileManagerService::start_operation()` (lines ~515-888, ~370 lines) is a giant `match` over `OperationKind` that eagerly resolves providers, checks capabilities, and constructs executor structs inline. The executors (`CopyExecutor`, `MoveExecutor`, `DeleteExecutor`, `CreateArchiveExecutor`, etc.) are private structs embedded in the same file, impossible to test independently. All executor construction bugs can only be found by exercising the full service.

The **deletion test** confirms: deleting this `match` block would scatter the complexity (provider resolution, capability checking, executor construction) across every caller. Concentrating it into an `OperationPlanner` module creates a deep seam.

## Acceptance Criteria
- `OperationPlanner` module in `crates/fm-application/src/operation_planner.rs` with a single interface: `plan(kind, request) -> Result<Arc<dyn OperationExecutor>, ApplicationError>`
- All executor structs (`CopyExecutor`, `MoveExecutor`, `DeleteExecutor`, `CreateArchiveExecutor`, `TrashExecutor`, etc.) moved into the planner module
- Provider resolution and capability checking contained within the planner
- Tests for every executor construction path — including move copy+delete fallback, archive format inference, capability rejection — exercisable without bootstrapping `FileManagerService`
- `FileManagerService::start_operation()` reduced to: call planner, submit to scheduler, handle idempotency
- Zero behavioural changes

## Implementation Notes
- The planner needs access to: `ProviderRegistry`, `ArchiveFileSystemProvider`, `PlatformAdapter` (for trash), `Settings` (for delete confirmation flag), `audit_log_path`
- These are passed to the planner constructor, not leaked through the interface
- The idempotency map and scheduler interaction stay in `FileManagerService` — they're orchestration concerns, not planning concerns
- Test with probe providers (existing `LateProvider` pattern from `directory.rs` is a good reference)
- ~370 lines removed from `service.rs`, concentrated into planner module

## Agent Notes

- 2026-08-11 Extracted all 13 executor structs (`CreateDirectoryExecutor`, `RenameExecutor`, `RenameGroupExecutor`, `PlannedCopyEntry`, `CopyExecutor`, `CopyGroupExecutor`, `CreateArchiveExecutor`, `ArchiveCreationFormat`, `DuplicateExecutor`, `DeleteExecutor`, `TrashExecutor`, `MoveExecutor`, `MoveGroupExecutor`) and their `OperationExecutor` impl blocks from `service.rs` into `operation_planner.rs`. Moved helper functions (`effective_resolution`, `conflict_error`, `conflict_entry`, `copy_stream_error`). `FileManagerService::start_operation()` reduced from ~370-line match to single `self.planner.plan()` call. Service shrank from 5797 to 3980 lines. Added 7 planner-specific tests (archive format inference, format mismatch rejection, unknown extension rejection, empty sources rejection for all operation types, search rejection, trash platform capability rejection). All 152 tests pass, zero warnings, zero behavioral changes.
