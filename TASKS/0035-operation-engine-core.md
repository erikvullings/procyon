# 0035 Operation engine core: jobs, scheduler, progress

Status: done
Priority: high
Owner: unassigned
Agent: unassigned
Area: backend
Depends on: 0031, 0018

## Context
`file-manager-coding-agent-spec.md` §17 and §33 step 7. Every mutating operation is a job owned by
the Rust engine (§3 rules 6 and 8). This task builds the machinery; individual operation kinds land
one at a time in 0037–0044.

## Acceptance Criteria
- `fm-operations` defines `Operation`, `OperationState`, `OperationKind`, `OperationProgress` and
  `ConflictPolicy` exactly as in §17.
- A scheduler runs operations with configurable concurrency (from settings), tracks state
  transitions, and rejects illegal transitions with a typed error.
- Two phases per operation: planning (enumerate work, compute totals) then execution, so
  `total_items`/`total_bytes` are known before progress is reported where feasible.
- Progress events are throttled/coalesced (e.g. at most ~10/s per operation) before publication
  (§28).
- `bytes_per_second` is a smoothed rate, not an instantaneous spike.
- Every operation is cancellable at a safe point; cancellation leaves no partial destination file
  (see 0046 for the full cancellation surface).
- Safety pre-checks shared by all operations (§17): source == destination, destination inside
  source, case-only differences on case-insensitive filesystems, symlink cycles, and a refusal to
  replace a directory with a file or vice versa.
- Operations are published as `operation.created` / `operation.stateChanged` / `operation.progress`
  / `operation.completed` / `operation.failed` events.
- Unit tests for state machine transitions, planning totals, throttling, and each safety pre-check.
- No operation kind is executable yet — a `NotImplemented` operation kind is fine for testing the
  scheduler.

## Implementation Notes
- Do not implement all operations in one unreviewable change (§33 step 7).
- Structured tracing per operation with `operation_id` (§30).
- Design the plan step so it can later be persisted for crash-safe history (§37).

## Agent Notes

- 2026-07-31 codex: Added the exact §17 operation model, typed lifecycle transition errors, a
  settings-configurable bounded scheduler, materialized planning totals, cooperative cancellation
  with partial-destination cleanup, 10 Hz coalesced progress, exponentially smoothed transfer
  rates, structured `operation_id` tracing, and all required operation lifecycle events.
- 2026-07-31 codex: Added shared preflight checks for same and nested destinations, case-only
  differences on case-insensitive filesystems, provider-supplied symlink-cycle identities, and
  file/directory replacement mismatches. Concrete operation kinds remain unimplemented and plug in
  only through the typed planning/execution boundary owned by tasks 0037–0044.
- 2026-07-31 codex: Added 11 task-specific public-contract tests in `operation_engine.rs`, including
  realistically interleaved planning input, scheduler concurrency/event sequencing, and safe-point
  cancellation cleanup. Verified with `cargo test -p fm-operations --test operation_engine`,
  `cargo check -p fm-operations --all-targets`, `cargo test -p fm-operations`, workspace formatting,
  strict workspace Clippy, and the full `pnpm test` suite. `CLAUDE.md` does not exist. Platform path
  comparisons were executed on macOS; Windows case-insensitive behavior is exercised through the
  explicit filesystem-sensitivity input rather than on a Windows host.
