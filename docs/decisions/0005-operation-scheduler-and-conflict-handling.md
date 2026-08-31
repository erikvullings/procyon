# 0005 Operation scheduler and conflict handling

Status: accepted

## Context
File operations (copy, move, delete, rename, archive extraction, ...) can be long-running, can
conflict with concurrent operations or filesystem changes, and must be resumable/cancellable rather
than blocking a request/response cycle (spec §3 rule 6, §19 operations model).

## Decision
`fm-operations` owns an operation scheduler: every file operation becomes a job with its own
lifecycle (queued, running, paused, completed, failed, cancelled), reported to clients through the
event bus (ADR [0002](0002-axum-rest-and-sse.md)) rather than as a single synchronous HTTP
response. Conflicts (e.g. destination exists, source changed mid-copy) are resolved through an
explicit conflict-resolution step the job pauses on, not by the engine silently choosing a
strategy. `fm-operations` sits below `fm-application` and above the VFS trait (ADR 0004), so it can
orchestrate cross-provider operations without depending on Axum or Tauri (rule 4).

## Alternatives
- **Synchronous request/response per operation**: rejected — violates rule 6 directly, and does not
  support progress reporting, cancellation, or pause-for-conflict-resolution for large operations.
- **Client-side operation orchestration** (frontend loops over individual file copies): rejected —
  violates rule 8 (frontend must not implement file-copy semantics) and rule 7 (backend must own
  authoritative operation state).
- **Silent conflict policy (always overwrite / always skip)**: rejected — spec §35 explicitly
  forbids silently overwriting user files.

## Consequences
- Every operation-initiating endpoint/command returns a job handle immediately; callers poll or
  subscribe to events for progress rather than waiting on the call.
- Conflict resolution needs its own DTO and UI (a dialog, not an error), and the operation must be
  able to pause/resume around it.
- Cancellation must be threaded through the scheduler and down into the VFS calls it drives
  (AGENTS.md: "add cancellation to long-running work").

## Revisit conditions
Revisit if job persistence (surviving a server restart mid-operation) becomes a requirement, or if
the conflict-resolution UX needs to support batch/"apply to all remaining" decisions the current
per-conflict model doesn't cover.
