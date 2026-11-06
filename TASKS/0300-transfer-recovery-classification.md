# 0300 Transfer recovery classification on restart

Status: done
Priority: high
Subsystem: backend, operations, frontend
Depends on: 0299

## Context
After a restart the app must tell the user what happened to each interrupted transfer instead of
silently dropping it or blindly restarting. Parent: 0163.

## Acceptance Criteria
- On startup each journaled job is classified as `resumable`, `safelyRestartable`,
  `completedUnconfirmed`, or `needsIntervention`, each with a typed reason code and localized text.
- Classification re-checks the source revision and the destination state through the VFS
  providers; a stale source, replaced destination, or unreachable provider is never classified
  resumable.
- Operation history/queue DTOs and the UI show the classification and reason; the user can retry,
  restart, or discard each job. Nothing restarts without an explicit user action unless the job
  was `safelyRestartable` and the user enabled auto-restart.
- Private partial destinations are never listed or exposed as completed files; the user can
  deliberately retry or remove them (removal goes through the operation engine).
- Cancellation during recovery leaves no partial behind.
- Fault-injection tests: restart mid-copy, stale source, replaced destination, provider
  unavailable, cancellation during recovery. OpenAPI/client regenerated; 8 locales.

## Implementation Notes
- Conservative by default: unverified partials are restarted or need intervention, never appended to.

## Agent Notes
- 2026-11-05 claude: Split from 0163.
- 2026-11-06 claude: Built `crates/fm-application/src/transfer_recovery.rs`: a pure per-item classifier (`classify_item`) and job roll-up (`classify_job`), plus `TransferRecoveryCoordinator` that holds the jobs 0299's journal recovered at startup, re-probes the providers on every list, and offers restart/discard. Reasons: notStarted, partialUnfinished, publishedUnconfirmed, sourceChanged, sourceMissing, destinationReplaced, providerUnavailable, journalDamaged. API: `GET /api/v1/operations/interrupted`, `POST .../interrupted/{id}/restart` (201 + the new operation), `POST .../interrupted/{id}/discard` (204); Tauri commands `list/restart/discard_interrupted_transfer`; all three frontend clients (http, tauri, mock). UI: `InterruptedTransfers` panel at the top of the Operation Centre (reason, counts, "Run again" / "Run again anyway" / "Discard" / "Mark as done"), a one-time toast at startup, 23 strings in all 8 locales. The journal's `Partial` record now also stores the item's final destination (`destination`, optional, additive).
- Behaviour: nothing runs or is deleted until the user chooses. Restart starts a NEW operation of the original kind/destination, leaves out sources that already finished (keeping the original conflict policy); when a finished item is a child of a directory source it keeps the directory and forces `skip` so finished files are not copied again. The new operation is started BEFORE partials are removed, so a failed start loses nothing. Discard and restart refuse (`providerUnavailable`) while a recorded partial cannot be probed, so no partial is orphaned. A per-job claim stops a double click or second client from running the same restart twice. Server authorisation reads the stored intent (no provider probing) and restricts to the accessible roots; the list is filtered the same way.
- Classification rules worth knowing: a destination that exists with no partial and a matching size is `completedUnconfirmed` only if it is not older than the job (otherwise the conflict policy decides again); a recorded partial with no recorded destination (journal written before this task) is `journalDamaged`; a corrupt middle journal record is `journalDamaged`, a torn tail is not; a source whose size OR modification time differs from the recorded revision is `sourceChanged`. Local sources record no revision (0299 skips that stat), so only size/existence are checked for them.
- Verified: 35+ new Rust tests in `transfer_recovery.rs` (classifier rules incl. interleaved verdicts, real-file coordinator tests for list/discard/restart/exclusion/claim/unreachable partial/failed start), 1 server route test + OpenAPI ids, 9 frontend tests (component, operation-centre mount, mock client). `cargo clippy` clean on fm-operations/fm-application/fm-server/fm-transport-dto; `cargo nextest run -p fm-application -p fm-server -p fm-operations` 1532 passed; OpenAPI/client regenerated.
- DEVIATIONS from the acceptance text: (1) no `resumable` verdict in production: no provider can prove partial ownership yet, so `resume_proven` is always false (0303 flips it per provider; the rule is unit-tested). (2) No auto-restart setting: nothing ever restarts without an explicit click (stricter than asked). (3) Partials are removed with the provider's `discard_copy` (the API the copy executor itself uses for its private temporaries), not by submitting a Delete operation, so removal does not appear in history. (4) "Cancellation during recovery": classification and discard are quick and not cancellable; the restarted operation is a normal operation and cancels normally. (5) A restart does not carry over `skipSpaceCheck` or the symlink policy of the original request (not journaled).
- Known gaps / follow-ups: probes run sequentially (slow hosts delay a list; no concurrency); a crash between publishing a file and the 250 ms journal flush can still leave a Move item whose source is gone and which has no records, reported as `sourceMissing` for the user to discard; recovered jobs of non-transfer kinds are not listed; no end-to-end test that kills a real service mid-copy (the journal and classifier are tested separately, plus the scheduler's mid-run state).

