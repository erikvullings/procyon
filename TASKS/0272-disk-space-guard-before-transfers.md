# 0272 Disk-space guard before transfers

Status: open
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
- Created 2026-10-18 from the media/playlist feature discussion. No work started.
