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
- `OperationKind::Delete` removes files and directory trees recursively, with planning-phase counts
  so the confirmation dialog can state exactly what will be deleted.
- A confirmation dialog is mandatory unless the user has explicitly disabled the confirm-permanent-
  delete setting; the dialog states the item count, total size and the fact that it is irreversible,
  and defaults to cancel.
- Symbolic links are removed, never followed into their target (§35).
- Read-only entries require an explicit override rather than being force-deleted silently.
- Cancellation stops between entries; the result reports exactly what was deleted.
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
