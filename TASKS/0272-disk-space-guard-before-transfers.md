# 0272 Disk-space guard before transfers

Status: done
Priority: medium
Subsystem: backend, frontend
Depends on: 0096, 0035

## Context
A copy or move that cannot fit on the target volume currently fails midway. Check capacity up front and warn.

## Acceptance Criteria
- Planning computes required bytes (accounting for same-volume moves and overwrites) and compares with the target volume capacity (0096); insufficient space shows a clear dialog with the shortfall before any data is written.
- User can proceed anyway for cases where the estimate is uncertain (compression, sparse files, cloud providers); the decision is recorded in the operation.
- Providers without capacity information skip the check silently; tests with a fake provider report capacity.

## Implementation Notes
- Hook into the operation planner (`operation_planner.rs`), not the frontend.

## Agent Notes
- Created 2026-10-18 from the media/playlist feature discussion. 
- 2026-10-18 claude: The preflight already existed in CopyExecutor::plan (hard InsufficientSpace); this task refines it in operation_planner.rs: (1) verify_capacity/net_required_bytes - when gross bytes exceed free space the requirement is refined for the conflict policy (Overwrite: only growth over the existing file; Skip: nothing; Ask/RenameNew: full, conservative); (2) multi-source copy and move are checked once on the combined plan (copy/move group executors; individual executors skip their own check); same-volume moves are renames with 0 bytes and fallback copies only are counted; (3) unknown capacity/total silently skips (test with fake provider: capacity None); (4) StartOperationRequestDto.skip_space_check lets the user proceed: the shortfall is recorded as a CompletedWithWarnings entry 'Disk-space check skipped at your request: N bytes needed, M free' on the first completed item (persisted in operation history errors). operation.failed events now carry structured details {required, available} (fm-events/fm-operations scheduler). Frontend: InsufficientSpaceDialog opens when a copy/move/duplicate fails with insufficientSpace (shows required/available/shortfall + uncertainty note); Proceed anyway re-submits the same operation with skipSpaceCheck. Nothing is written before the dialog (failure happens in planning). The failed first attempt stays in the operation list until dismissed by the retry. Tauri parity automatic (same DTO/event). Not covered: moves server-side on remote providers are not counted.
