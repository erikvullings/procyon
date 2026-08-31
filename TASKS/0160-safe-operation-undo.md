# 0160 Safe operation undo

Status: open
Priority: high
Subsystem: backend, frontend, operations
Depends on: 0035, 0043, 0047

## Context

Procyon's operation history reports completed work but cannot reverse it. Safe undo for rename,
move, copy, duplicate, and trash operations would make destructive workflows substantially more
forgiving. Undo must never overwrite later user changes or imply reversibility where the original
operation did not retain enough evidence.

## Acceptance Criteria

- Operation history identifies completed operations that are currently undoable and explains why an
  operation is not undoable.
- Rename and move undo restore the original location only when source and destination revisions
  still match the completed operation.
- Copy and duplicate undo remove only entries created by that operation, after verifying their
  identity or content revision; modified outputs are never deleted automatically.
- Trash undo uses the platform's supported restore mechanism or a recorded original location. A
  permanent delete is never presented as undoable.
- Undo itself runs as an operation-engine job with progress, cancellation, conflicts, audit history,
  and HTTP/Tauri parity.
- Restarting the application preserves the evidence needed to undo operations that remain safe.
- Tests cover changed/missing destinations, reused paths, cross-provider moves, partial failure,
  cancellation, restart, and an attempt to undo an already-undone operation.

## Implementation Notes

- Persist an explicit undo plan and source/destination fingerprints with the history record rather
  than trying to reconstruct intent later from display text.
- Use provider revisions or stable identities where available and conservative metadata/content
  verification otherwise. If safety cannot be established, refuse the undo with a useful reason.
- Do not implement undo as a frontend-only inverse action; all mutations remain authoritative in the
  operation engine.

## Agent Notes

- 2026-08-28: Created from the product feature review. Recommended as the highest-value next feature
  because it deepens Procyon's existing safety model rather than adding another isolated tool.
