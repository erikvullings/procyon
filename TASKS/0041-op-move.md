# 0041 Operation: move files and directories

Status: done
Priority: high
Owner: unassigned
Agent: unassigned
Area: backend
Depends on: 0040

## Context
`file-manager-coding-agent-spec.md` §33 step 7 item 5, §16 milestone 2 and §17 (cross-volume moves).

## Acceptance Criteria
- `OperationKind::Move` uses a same-volume rename when possible, and falls back to copy-then-delete
  across volumes.
- The copy-then-delete fallback deletes the source only after the destination is fully written and
  verified to exist; a failure mid-way never loses the source (integration test asserts).
- Cancelling a cross-volume move leaves the source intact.
- Rejects destination-inside-source and source==destination (§17).
- Moving a directory onto an existing directory is never a silent merge-or-replace; the behaviour is
  explicit and driven by the conflict policy.
- `F6` moves the selection to the other pane.
- Integration tests: same-directory move, cross-directory move, simulated cross-volume move,
  cancellation during the fallback path, directory move with open children, Unicode names.

## Implementation Notes
- Simulate the cross-volume path in tests by forcing the fallback via a test-only flag rather than
  requiring a second real volume — document that the real cross-volume path is only exercised
  manually (§35: report platform-untested behaviour explicitly).
- Reuse 0039/0040 for the copy half; do not fork the logic.

## Agent Notes
- 2026-07-31: Added multi-selection move, same-filesystem rename, and strict recursive
  copy/verify/delete fallback. Integration tests force the fallback and prove collision failure and
  cancellation retain the source. F6 targets the other pane.
- 2026-07-31: The real cross-volume path is platform-untested as requested; tests use the documented
  test-only fallback flag.
