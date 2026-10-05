# 0237 Tolerate per-file failures during semantic reconciliation

Status: done
Priority: high
Subsystem: backend, rag
Depends on: none

## Context

A full enrolled-root reconciliation aborts on the first provider error. On OneDrive, reads of
cloud-only or slow files regularly time out, so a full pass never completes, no
`indexed_generation` is ever recorded, and the UI cannot tell an indexed library from an
unindexed one. `zvec-grep` records per-file indexing failures in its index and keeps going.

## Acceptance Criteria

- A read failure for one file, or a listing failure for one sub-directory, is counted and logged
  but does not abort the pass. Cancellation, worker/transport failures, limits, and a failure to
  list the root itself still abort as before.
- Previously indexed occurrences of failed files, and of everything beneath a failed directory,
  are preserved: `complete_reconciliation` must not treat them as deleted.
- A pass with only tolerated failures commits a new reconciliation generation and reports the
  unreadable file/directory counts.

## Implementation Notes

- Reconcile loop: `crates/fm-application/src/semantic_indexing.rs`.
- Preservation is resolved inside the catalog lock (`fm-semantic-library` catalog) using
  `is_same_or_descendant`, so no host-side snapshot can race a concurrent catalog change.

## Agent Notes
- 2026-10-04 Copilot: Done. Entry-local provider failures (`NotFound`, `PermissionDenied`, `Locked`,
  `Io`, `LinkCycle`, archive limits, ...) on a file read or a non-root directory listing are counted
  in `SemanticIndexingReport::unreadable_files` / `unreadable_directories` and their locations
  passed to the new `complete_reconciliation_preserving`, which keeps occurrences at or below them
  while still committing the generation. Root listing failures, cancellation, authentication,
  worker and limit errors still abort. Counts are summed into the model-change report and logged by
  the desktop startup reconcile. Covered by an integration test using `chmod 000` (Unix only);
  real OneDrive timeouts were not reproduced.
