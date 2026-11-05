# 0301 Bounded retry with exponential backoff for transfers

Status: open
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
