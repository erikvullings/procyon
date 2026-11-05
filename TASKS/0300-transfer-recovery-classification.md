# 0300 Transfer recovery classification on restart

Status: open
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
