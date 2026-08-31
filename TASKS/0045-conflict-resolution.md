# 0045 Conflict detection, policies and resolution dialog

Status: done
Priority: high
Owner: unassigned
Agent: unassigned
Area: backend
Depends on: 0041

## Context
`file-manager-coding-agent-spec.md` §17 ("conflict policies") and §10 (conflict resolution is an SSE
event in, a REST/Tauri decision out). §35: never silently overwrite user files.

## Acceptance Criteria
- `ConflictPolicy` supports `Ask`, `Skip`, `Overwrite`, `RenameNew` reliably; `KeepNewer` may be
  declared but must either work or be rejected as unsupported — never silently behave as another
  policy (§17).
- With `Ask`, the operation transitions to `WaitingForConflictResolution`, emits
  `operation.conflict` with both entries' name, size and modified time, and blocks that item only —
  other queued work continues where safe.
- The frontend conflict dialog offers: overwrite, skip, rename, cancel operation — each with
  "apply once" and "apply to all similar conflicts" (§17).
- The decision is submitted through `resolveOperationConflict` (REST) or the Tauri command, never
  through the event stream (§10).
- A directory is never replaced by a file or a file by a directory, regardless of policy (§17, §35).
- Conflicts detected after planning (destination appearing late) are handled the same way as
  conflicts found during planning.
- Timeouts/disconnects while waiting do not lose the operation; on reconnect the pending conflict is
  re-presented.
- Integration tests per policy, plus: apply-to-all, cancel from the dialog, late-appearing
  destination, directory-vs-file mismatch.
- Vitest tests for conflict-dialog state machine (§27).

## Implementation Notes
- `RenameNew` reuses the duplicate naming function from 0042.
- Keep the pending-conflict registry in the engine so both transports see identical behaviour.

## Agent Notes
- Not started.
- 2026-07-31: Added a typed frontend conflict-dialog state machine with Vitest coverage for
  overwrite, skip, rename, cancel, and apply-to-all state; added a reusable conflict dialog
  component and conflict entry metadata fields to the frontend event model. `KeepNewer` is now
  explicitly rejected by the application service as unsupported. Focused frontend tests,
  frontend typecheck, `fm-application` library tests (76), and `fm-operations` tests (1) pass.
  The task remains open: the engine still needs a pending-conflict registry, per-item conflict
  events with source/destination metadata, decision application through REST/Tauri, late-conflict
  handling, and backend policy integration tests. Full frontend verification is currently
  affected by pre-existing theme-test and sandbox localhost-listen failures.
- 2026-07-31: Completed the engine-owned pending-conflict flow. Conflicting items are deferred
  while safe planned items continue, requested one-shot or apply-to-all decisions are consumed by
  copy/move executors, and reconnecting REST/Tauri subscribers receive the pending conflict again.
  Conflict events now carry both entries' kind, name, size, and modified time. Late collisions,
  cancel, skip, overwrite, rename-new, directory/file mismatch, and cross-volume source safety are
  covered by integration tests; KeepNewer remains explicitly unsupported. Rust workspace lint,
  all focused operation/application tests, all fm-server tests, script tests, frontend typecheck,
  and the conflict-dialog Vitest suite pass. The full frontend suite has one unrelated pre-existing
  failure in `features/panes/pane.css` for a hard-coded colour; 250 other tests pass.
