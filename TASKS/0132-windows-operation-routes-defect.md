# 0132 Windows defect: operation routes return 500 / deadlock

Status: done
Priority: high
Owner: unassigned
Agent: unassigned
Area: server,operations
Depends on: 0035, 0045

## Context
Surfaced while manually verifying task 0060 ("Windows platform integration") on a real Windows
machine. `apps/fm-server`'s `tests/operation_routes.rs` integration suite has three failures that
are specific to Windows and were confirmed to reproduce **identically with the 0060 changes
stashed** — i.e. this is a pre-existing defect in the operation engine / server routes, unrelated
to the Windows platform adapter work, not something 0060 introduced. It currently blocks the
pre-commit hook (`scripts/pre-commit.mjs`'s `cargo test` step) on Windows.

Failing tests (`apps/fm-server/tests/operation_routes.rs`):
- `resolve_conflict_route_applies_the_requested_decision` — returns HTTP 500 instead of applying
  the conflict decision.
- `resolve_conflict_route_confirms_a_permanent_directory_delete` — same, HTTP 500 on a
  conflict-confirmed permanent directory delete.
- `start_retry_uses_stable_id_and_copy_emits_full_lifecycle` — deadlocks (test exceeds a 60s
  timeout) rather than completing the copy lifecycle.

All three exercise the real operation engine end-to-end (spawn a `TestServer`, POST to
`/api/v1/operations`, poll/resolve via the REST routes, assert on `SubscriptionEvent`s from the
event bus) against real temp-directory fixtures, so the failures are in actual Windows filesystem
behavior interacting with the operation engine (`crates/fm-application`'s operation
planner/executor) or its locking/conflict-resolution path — not test-harness flakiness.

## Acceptance Criteria
- Root-cause each of the three failures on Windows: a plausible starting hypothesis is Windows
  path/locking semantics (e.g. a source or destination handle held open by one step of the
  operation preventing a later step from completing, causing both the 500s during conflict
  resolution and the retry deadlock) but this needs to be verified on real Windows hardware or CI,
  not assumed.
- `resolve_conflict_route_applies_the_requested_decision` and
  `resolve_conflict_route_confirms_a_permanent_directory_delete` pass on Windows, returning the
  expected 200/updated operation state instead of 500.
- `start_retry_uses_stable_id_and_copy_emits_full_lifecycle` completes and passes on Windows
  without deadlocking.
- A regression test (or the existing tests, once fixed) runs in CI's `Rust (windows-latest)` job so
  this cannot silently regress.
- The Windows pre-commit hook's `cargo test` step is unblocked.

## Implementation Notes
- Reproduce first with `cargo test -p fm-server --test operation_routes` on Windows (or CI) before
  changing anything; capture full output including any lock/handle related OS errors.
- Check `apps/fm-server` startup uses this crate's Windows-specific config already added by 0060
  where relevant (session auth, accessible roots) — this looks unrelated to those, but rule it out.
- If the root cause turns out to be a genuine cross-platform locking bug in the operation engine
  (e.g. a handle not dropped before a dependent step runs), fix it generally rather than special-
  casing Windows, since a real bug here can also bite `fm-vfs-local` on network filesystems on
  other platforms.

## Agent Notes
- Verified resolved on Windows 11 after the task 0060 merge (`f72976c`, now on `main`). The
  Windows-specific test fixture URI changes in that merge use `Location::from_native_path`, so
  the operation routes receive valid `file:///C:/...` locations rather than invalid
  `file://C:\...` values. The resulting invalid-location errors were the cause of the reported
  HTTP 500 conflict-resolution responses and stalled copy lifecycle.
- `cargo test -p fm-server --test operation_routes -- --nocapture` passes all four tests on
  Windows, including both conflict resolutions and the idempotent copy lifecycle.
- `cargo test --workspace` also passes on Windows, so the pre-commit hook's Rust test step is no
  longer blocked. The existing route integration tests run in the `Rust (windows-latest)` CI job.
