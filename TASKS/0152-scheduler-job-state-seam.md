# 0152 Give Scheduler::run_job an atomic interruption-state seam

Status: done
Priority: medium
Subsystem: backend
Depends on: none

## Context

Found via `/improve-codebase-architecture`. `Scheduler::run_job` in
`crates/fm-operations/src/scheduler.rs` (846 lines total, 13 `pub fn`, **zero tests**) is a single
async function (~195 lines, roughly lines 403–597) interleaving permit acquisition, planning,
confirmation-wait, per-item execution, conflict deferral/retry, cancellation checks, and progress
publishing — all against a `Job` struct with 6 separately-locked `Mutex`/`Notify` fields
(`operation`, `pending_conflict`, `apply_to_all`, plus two `Notify`s).

Cancellation is checked via `job.cancellation.is_cancelled()` at roughly 7 different points
scattered through the loop, each independently racing the next lock acquisition on a different
field — there is no single point that atomically reads "am I cancelled + what's my current state."
This is the riskiest concurrency logic in the operations engine (owns pause/resume/cancel/
conflict-resolution semantics for every mutating file operation) and it currently has no unit
tests at all — only reachable today via full integration tests that spin a real scheduler and
executors.

This is implementation/testability friction, not a reason to reopen
[ADR 0005](../docs/decisions/0005-operation-scheduler-and-conflict-handling.md) (operation
scheduler design) — the scheduler's overall shape stays; this is about how `Job`'s interruption
state is read and mutated internally.

## Acceptance Criteria
- `Job`'s interruption state (cancelled / paused / has-pending-conflict) is read and transitioned
  through one seam (e.g. a single state enum behind one lock, or an explicit state-transition
  method) instead of 6 independently-locked fields checked ad hoc through `run_job`'s loop.
- That seam is unit-testable without a real tokio scheduler/executor — e.g. constructing a `Job`
  and asserting state transitions and precedence (cancel-while-paused, conflict-while-cancelling,
  etc.) directly.
- `run_job`'s control flow reads the new seam instead of the old scattered checks; no behavioural
  change to job execution, pause/resume/cancel, or conflict handling visible to callers.
- Existing integration tests for operations (`cargo test -p fm-operations`, plus any
  `fm-application` operation tests) still pass.
- Zero compiler warnings; `cargo clippy -p fm-operations --all-targets -- -D warnings` clean.

## Implementation Notes
- Start by enumerating every read/write site of `job.cancellation`, `job.pending_conflict`,
  `job.apply_to_all`, and `job.operation` inside `run_job` and the rest of `scheduler.rs` before
  designing the new type — don't assume the 7 cancellation-check sites found during exploration are
  the complete list.
- Consider whether pause/resume, cancel, and conflict-resolution are genuinely one state machine
  (mutually exclusive states) or independent orthogonal flags — that determines whether one enum or
  a small set of atomically-paired fields is the right shape.
- Deletion test passed during exploration: deleting `run_job`'s current shape wouldn't remove
  complexity, it would force every VFS operation crate to reimplement job bookkeeping — so this is
  a real seam worth deepening, not a pass-through to strip out.

## Agent Notes
- 2026-08-25: Task created from `/improve-codebase-architecture` findings (candidate 2). Not yet
  investigated further beyond the initial Explore pass — see Implementation Notes for the first
  concrete step.
- 2026-08-26: Implemented the seam in `crates/fm-operations/src/scheduler.rs`. Enumerated every
  site touching `job.operation`, `job.pending_conflict`, and `job.apply_to_all` via
  `grep -n '\.cancellation\|\.pending_conflict\|\.apply_to_all\|\.pause\b\|\.resumed\b\|job\.operation\b'`
  (about 45 sites across `submit`, `get`, `list`, `pause`, `resume`, `confirm`, `resolve_conflict`,
  `republish_pending_conflicts`, `cancel`, `wait`, `run_job`, `wait_while_blocked`,
  `register_conflict`, `wait_for_conflict_decision`, `finish_cancelled`, `transition_job`, and
  `fail_job`) before designing.
  - Design decision: did *not* collapse everything into one enum. `Operation::state` is already a
    full lifecycle enum (Queued/Planning/Running/Paused/WaitingForConflictResolution/Cancelling/
    Cancelled/Completed/...) with its own `transition()` legality rules and is serialized to
    clients as-is — duplicating that into a second interruption-only enum would create two sources
    of truth that must stay in sync. Cancellation also needed to stay a `CancellationToken`: it is
    passed by reference into the public `OperationExecutor::plan`/`execute` trait methods
    (implemented outside this crate, e.g. `fm-application::operation_planner`), so its type is
    effectively a public contract, not an internal representation to refactor. Cancellation is also
    *not* mutually exclusive with paused/pending-conflict — it must override and unblock them
    (`cancel()` already called `pause.resume()` + `resumed.notify_waiters()` to wake a blocked job
    so it can observe cancellation), so folding it into one mutually-exclusive state machine would
    misrepresent its actual "always wins" relationship to the other two.
  - What *did* move: `Job` previously stored `operation: Mutex<Operation>`,
    `pending_conflict: Mutex<Option<PendingConflict>>`, and
    `apply_to_all: Mutex<Option<ConflictResolution>>` as three independently-locked fields, so
    "is this job blocked" required acquiring two of those locks non-atomically (see old
    `wait_while_blocked`, which read `operation.state` and `pending_conflict` in two separate
    critical sections). These three now live in one `JobState` struct behind Job's single
    `state: Mutex<JobState>`, so every read/mutation site gets one atomic snapshot. Added
    `JobState::blocking(&self) -> bool` as the one seam that decides "is this job blocked between
    units of work," and `Job::signal(&self) -> JobSignal { Proceed | Blocked }`, which layers the
    cancellation-always-wins precedence over it in one place instead of the ~7 scattered
    `job.cancellation.is_cancelled()` checks each separately racing a lock acquisition.
    `wait_while_blocked` now just loops on `job.signal()`. `CancellationToken` and `PauseToken`
    remain separate fields on `Job` (unrelated concerns: token-based cooperative signals consumed
    by the executor trait, not lifecycle state), and `completed`/`resumed` remain separate
    `Notify`s (they wake different waiters — job completion vs. unblock — and conflating them would
    cause spurious wakeups).
  - `run_job` and every `Scheduler` method were rewritten to go through `job.lock()` /
    `job.snapshot()` / `job.is_cancelled()` / `job.signal()` instead of the old per-field locking;
    no control-flow branch, transition target, or event-publish call changed — this is a pure
    internal locking/representation refactor, not a behavior change.
  - Added `#[cfg(test)] mod job_state_tests` at the bottom of `scheduler.rs`: constructs a bare
    `Job`/`JobState` from a plain `Operation` (no `Scheduler`, no executor, no tokio runtime needed
    for the assertions) and checks `blocking()`/`signal()` precedence directly, including
    cancel-while-paused and cancel-while-pending-conflict (cancellation token fired without yet
    transitioning `operation.state` away from `Paused`/`WaitingForConflictResolution` — confirms
    `Job::signal()` overrides the block), and the case where `cancel()`'s own transition to
    `Cancelling` also independently unblocks (`blocking()` returns `false` once state is
    `Cancelling`, regardless of a still-set `pending_conflict`).
  - Verified locally: `cargo check -p fm-operations --tests` passed clean (0 warnings/errors) after
    a full cold-cache dependency rebuild (~18m39s, mostly tokio/chrono/ciborium/etc., not this
    crate). Kicked off `cargo test -p fm-operations --lib` but the coordinating session picked up
    verification directly before it returned output — per this task's process constraints, did not
    run `cargo clippy`, `cargo fmt --check`, the full workspace build, or the integration test
    suite (`tests/operation_engine.rs`); leaving those to the coordinating session as instructed.
  - Status left as `open` — coordinating session to flip after full clippy/fmt/build/integration
    verification passes.
- 2026-08-26 (coordinating session): full verification surfaced a real, subtle correctness bug in
  the subagent's `blocking()` design, caught only by the integration suite (unit tests were
  self-consistently wrong, not just incomplete — see below):
  - `cargo clippy`/`cargo fmt` passed clean. But `cargo test -p fm-application --test
    conflict_resolution` failed 2/5: `skip_and_apply_to_all_leave_every_existing_file_unchanged` and
    `a_destination_appearing_after_planning_is_resolved_like_an_initial_conflict` both hung with the
    operation permanently stuck at `WaitingForConflictResolution`.
  - **Root cause**: `blocking()`'s original design blocked `WaitingForConflictResolution` when
    `pending_conflict.is_some()` — inverted from the *actual* required semantics. That state is
    reused for two different waits: (a) the initial "waiting for user confirmation before
    executing" gate (`requires_confirmation()`, no conflict registered — must block), and (b) a
    per-item conflict registered mid-loop, which `run_job`'s main `for item in &plan.items` loop
    *defers* (pushes to a `deferred` Vec and `continue`s to the next item) rather than blocking on —
    actual resolution happens later, only in the dedicated deferred-items pass via
    `wait_for_conflict_decision`. With the inverted condition, `wait_while_blocked` at the top of
    the per-item loop saw the just-registered conflict's `pending_conflict.is_some()` and blocked
    forever right there — deadlocking, since nothing on that path was waiting to resolve it (only
    the deferred-items pass, never reached, does). Confirmed by diffing against the pre-refactor
    `wait_while_blocked` in git history: its condition (`state != Paused && (state != Waiting ||
    has_item_conflict)`) returns/proceeds when a conflict *is* registered, and blocks specifically
    when Waiting *without* one — exactly backwards from the new code, and the reason the two
    conditions look confusable (double-negated boolean logic).
  - Fix: inverted the match arm to `WaitingForConflictResolution => self.pending_conflict.is_none()`
    and expanded the doc comment to state the invariant explicitly (why a registered conflict must
    NOT block here). The subagent's own `job_state_tests` had encoded the wrong semantics
    self-consistently (asserting `blocking() == true` when a conflict was registered) — passing
    unit tests were not evidence of correctness here, since the tests and the code shared the same
    inverted assumption. Rewrote the affected tests to assert the corrected behavior
    (`waiting_for_confirmation_without_a_registered_conflict_blocks`,
    `waiting_with_a_registered_item_conflict_does_not_block_the_main_loop`, plus a new
    `a_registered_item_conflict_never_blocks_regardless_of_cancellation`) and renamed
    `cancellation_overrides_a_pending_conflict_block`/`cancelling_the_operation_itself_also_unblocks_regardless_of_pending_conflict`
    to `..._confirmation_wait_block`/`..._a_confirmation_wait` since their premise (conflict = blocks)
    no longer held.
  - **Lesson for future sessions**: when a subagent's design inverts a subtle precedence rule, its
    own unit tests can pass while encoding the same inversion — they prove internal consistency, not
    correctness against the pre-existing system's actual behavior. The integration suite (which
    exercises real conflict-resolution end-to-end, not just the new type in isolation) was what
    actually caught this; skipping it to save time on a "mechanical" refactor would have shipped a
    deadlock.
  - Re-verified after the fix: `cargo test -p fm-operations --lib` (10/10, including the corrected
    tests), `cargo test -p fm-application --test conflict_resolution` (5/5), `cargo clippy -p
    fm-operations --all-targets -- -D warnings` (clean), `cargo fmt -p fm-operations -- --check`
    (clean), `cargo test -p fm-application --lib` (250/250), `cargo test -p fm-application` (full
    integration suite, all green), `cargo build --workspace` (clean). Committed as `<see git log>`.
  - Acceptance criteria met: the seam exists, is unit-testable without a real scheduler, `run_job`
    reads it exclusively, no behavioral change beyond fixing the bug this same pass introduced and
    caught, and the full verification chain (including the integration suite explicitly called out
    in the Acceptance Criteria) passes. Marking done.
