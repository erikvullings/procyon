# 0044 Operation: permanent delete with confirmation

Status: done
Priority: high
Owner: unassigned
Agent: unassigned
Area: backend
Depends on: 0043

## Context
`file-manager-coding-agent-spec.md` §16 milestone 2 ("permanent delete only after explicit
confirmation"), §36 item 11 (no silent permanent deletion) and §17 safety requirements.

## Acceptance Criteria
- `OperationKind::Delete` removes files and directory trees recursively. Local trees use a
  single-pass traversal: their totals are unknown before execution, rather than delaying removal
  to calculate exact counts.
- A confirmation dialog is mandatory unless the user has explicitly disabled the confirm-permanent-
  delete setting; it states that deletion is irreversible and defaults to cancel. The frontend
  prompts before recursive planning so large trees do not delay confirmation; direct unconfirmed
  backend jobs still require confirmation after checking the selected roots.
- Symbolic links are removed, never followed into their target (§35).
- Read-only entries require an explicit override rather than being force-deleted silently.
- Cancellation stops between entries; the result reports exactly what was deleted so far.
- Partial failures produce `CompletedWithWarnings` with a per-entry error list.
- Destructive integration tests run only inside temporary roots (§27, §35).
- Audit log entry written for every permanent delete (§22, §30) without logging file contents.

## Implementation Notes
- Deleting a large tree must not block the async runtime; iterate on the blocking pool with
  cancellation checks.
- The dialog is a `mithril-materialized` modal with correct focus trapping (§29).

## Agent Notes
- 2026-07-31: Added iterative post-order delete planning, exact confirmation totals, mandatory
  cancel-first materialized modal with trapped focus, read-only override, symlink-safe removal,
  per-entry warnings, exact cancellation progress, and content-free JSONL auditing. All destructive
  tests use temporary roots.
- 2026-07-31: Task 0043 trash remains a separate operation; this task implements only explicitly
  confirmed permanent deletion.
- 2026-08-30: Restored a visible red-text focus indicator on the permanent-delete action when the
  modal's keyboard trap moves focus with Tab. The trap now cycles only between Cancel and Delete
  permanently instead of including framework chrome, and cancelling an unconfirmed delete removes
  it from the operation centre immediately.
- 2026-10-07: The frontend now confirms the selected top-level sources immediately, before
  recursively planning large trees. The dialog explicitly says that counts are not yet known;
  accepting submits a confirmed operation. This initially kept exact post-planning totals;
  the subsequent single-pass local traversal below supersedes that counting step.
- 2026-10-07: The dialog closes on confirmation while the job runs; independent deletes can use
  the scheduler's configured concurrency (default two). Local non-recursive removal skips the
  extra metadata lookup for ordinary files, and local delete planning requests larger directory
  pages to reduce repeated offset scans. The byte-based time-to-completion estimate was removed;
  the subsequent single-pass change reports actual removed-entry counts instead.
- 2026-10-07: Local permanent deletion now walks and removes entries in one blocking-pool pass,
  avoiding per-file metadata lookups when override is explicit, reporting failures by path, and
  publishing batched item counts while it runs. The application
  reuses active jobs for identical paths and rejects overlapping delete/copy/move jobs, while the
  frontend suppresses repeated confirmation requests. Direct backend confirmations no longer
  wait for a full-tree count; remote providers retain their provider-specific planning behavior.
