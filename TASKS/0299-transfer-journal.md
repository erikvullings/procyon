# 0299 Durable transfer journal

Status: open
Priority: high
Subsystem: backend, operations
Depends on: 0035, 0047

## Context
Queued and running transfer jobs must survive a crash or restart. Today only terminal snapshots are
persisted (`operation_history.rs`); a job that was queued or running when the process died simply
vanishes, and a private partial destination can be left behind with no record of whose it is.
Parent: 0163.

## Acceptance Criteria
- A crash-consistent journal (write temp + fsync + atomic rename, or append-only with checksummed
  records) persists, per queued/running transfer: job id, plan intent, source/destination
  locations, source revision (size, mtime, provider validator), conflict policy, destination
  partial path, and checkpoints (bytes committed per item).
- Journal stores intent and checkpoints only; never live provider/session objects or credentials.
- Writes stay off the scheduler's critical path (reuse the write-behind approach of
  `OperationHistory`), but a checkpoint is durable before the item is reported as committed.
- A torn or corrupt journal file is detected, quarantined, and never prevents startup.
- Terminal jobs are removed from the journal once the history entry is durable.
- Fault-injection tests: crash between checkpoint and rename, truncated file, corrupt record.

## Implementation Notes
- Define the record schema and versioning first; additive fields, explicit version.
- Keep it inside the operation engine path; the engine remains the sole mutation authority.

## Agent Notes
- 2026-11-05 claude: Split from 0163; this is the foundation the other subtasks build on.
