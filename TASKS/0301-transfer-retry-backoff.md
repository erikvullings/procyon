# 0301 Bounded retry with exponential backoff for transfers

Status: done
Priority: medium
Subsystem: backend, operations
Depends on: 0035

## Context
Transient transfer failures (disconnects, timeouts, 5xx/throttling) should be retried a bounded
number of times; permanent ones must surface. Parent: 0163.

## Acceptance Criteria
- A retry policy with bounded attempts, exponential backoff with jitter, and a maximum delay;
  cancellation interrupts a backoff wait immediately.
- A typed error classification (transient vs. permanent) shared by providers; authentication
  failures, host-key mismatches, permission and conflict errors are never retried indefinitely
  and are surfaced with their reason.
- Retry attempts and the final cause appear in operation progress/history.
- Retries never re-run a non-idempotent step against an unverified destination.
- Tests with a fake provider: transient then success, exhausted retries, auth/host-key/conflict
  surfaced immediately, cancellation during backoff, deterministic clock (no real sleeps).

## Implementation Notes
- Inject a clock/sleeper so tests are deterministic.

## Agent Notes
- 2026-11-05 claude: Split from 0163. Independent of the journal; can ship first.
- 2026-11-06 claude: `crates/fm-operations/src/retry.rs` (`RetryPolicy` with bounded attempts, exponential growth, cap and jitter; `Sleeper`/`JitterSource` seams; `TokioSleeper` whose wait is cancellable) and `Scheduler::execute_with_retry`. Defaults: 4 attempts, 500 ms first wait, x2, 30 s cap, +-20% jitter; `Scheduler::with_retry`/`with_retry_environment`. Classification is shared: `VfsError::is_transient()` (fm-vfs) is true only for provider `Io` failures, except messages naming a host key, authentication, or permission/access denial, which are held back; every other variant (authentication, credentials, permission, privilege, conflicts, missing entries, space, cancellation) is permanent. `ExecutionError::is_transient()` exposes it to the engine.
- Behaviour per retry: roll back the bytes the failed attempt reported, discard the item's private partial through `cleanup_partial`, wait (interruptible by cancel), record a `transferRetried` warning on the operation (attempt, max, seconds, cause), then run the item again. A retry whose partial cannot be removed surfaces the original error. The retry warnings are entry errors, so they appear in the Operation Centre and in persisted history (and a retried-then-successful job ends `completedWithWarnings`); the final cause of an exhausted budget is the normal operation failure. 8-locale text for `transferRetried`.
- Idempotency guard: executors opt in with `OperationExecutor::retry_safe` (default false). Opted in: `CopyExecutor`, `CopyGroupExecutor`, `DuplicateExecutor` (partial output is private, nothing published before the final rename). NOT opted in: move executors and every non-transfer executor, because copy-then-delete is not idempotent against an unverified destination.
- Verified: 7 unit tests in `retry.rs` (growth, cap, jitter bounds, nonsense settings, bounded attempts, cancellable wait), 6 scheduler tests in `tests/transfer_retry.rs` on a recording clock (transient then success with exact waits and cleanups, exhausted budget, 7 permanent error shapes never retried, not-retry-safe never re-run, cancel during backoff, disabled policy, bytes not double counted), 2 frontend tests for the localized text. `cargo clippy` clean; `cargo nextest run -p fm-vfs -p fm-operations -p fm-application` 1341 passed.
- Known gaps: the policy is not user-configurable (no setting yet); host-key and auth failures reach the VFS layer as `Io` text in some providers, so the hold-back is a message check, not a typed variant; moves are not retried; only the first execution pass of an item is wrapped (a conflict resolved later is not retried).
- 2026-11-06 claude: Review follow-ups applied: `VfsError::is_transient` is now an allowlist of network-shaped causes (reset/closed/aborted/refused connection, broken pipe, timeout, "temporarily", "too many requests", "service unavailable", unexpected EOF, "network") minus a blocklist (host key, authentication, permission/access denied, unauthorized, forbidden, not permitted), so an unclassified `Io` (bad sector, read-only filesystem) is never retried; wait text rounds up ("1 s", never "0 s"); a failed cleanup before a retry is logged; a pause requested during backoff holds the next attempt back.
- Known limits from review, left open: (a) the deferred pass that re-runs an item after the user resolves a conflict is not wrapped in retry; (b) an `Io` failure AFTER the final rename (a lost response on SFTP/FTP/WebDAV) is retried and then meets its own published output as a conflict (surfaced, never silently overwritten unless the policy is overwrite) because `cleanup_partial` only discards the temporary; (c) every retry adds one entry to `errors` (bounded in history by the existing omission logic) and a job that recovers ends `completedWithWarnings`; (d) a retry leaves the old partial's journal record behind, which recovery (0300) reads as an absent partial; (e) `RandomJitter` borrows the random bits of a generated id instead of a dedicated RNG.

