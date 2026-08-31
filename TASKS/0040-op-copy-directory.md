# 0040 Operation: copy a directory tree

Status: done
Priority: high
Owner: unassigned
Agent: unassigned
Area: backend
Depends on: 0039

## Context
`file-manager-coding-agent-spec.md` §33 step 7 item 4, §16 milestone 2 and §17 safety requirements.

## Acceptance Criteria
- Recursive copy driven by the engine's planning phase: enumerate the tree, compute total items and
  bytes, then execute — with the enumeration itself cancellable and progress-reported.
- Rejects destination-inside-source before any bytes are written (§17); integration test asserts.
- Symbolic links are not followed recursively by default (§6, §35); the policy (copy the link vs
  copy the target) is explicit in the request and documented.
- Traversal is cycle-protected, so a symlink loop cannot hang or explode the plan (§27).
- Directory metadata (timestamps, permissions where supported) applied after children are copied.
- Partial failures produce `CompletedWithWarnings` with a per-entry error list, not a silent success
  and not an abort of the whole tree (unless the user chose to abort).
- Cancellation mid-tree cleans up the in-flight file but leaves already-copied files in place, and
  the result clearly states the copy is incomplete.
- Deeply nested trees do not overflow the stack (iterative traversal).
- Integration tests: nested tree with Unicode names, empty subdirectories, symlink cycle, deep
  nesting, cancellation mid-way, destination inside source, 10,000 small files (perf fixture §28).

## Implementation Notes
- Reuse the single-file copy path from 0039 per file; do not duplicate the streaming logic.
- Concurrency across files is bounded by the settings-driven operation concurrency (0030).

## Agent Notes
- 2026-07-31: Added iterative recursive planning/execution, explicit copy-link/copy-target policies
  with stable-id cycle protection, post-order directory metadata, warning aggregation, and safe
  cancellation cleanup. Covered Unicode/empty trees, both symlink policies and a loop, deep nesting,
  destination-inside-source rejection, and the 10,000-file fixture in temporary roots.
- 2026-07-31: Local-provider native copy avoids per-file process spawning; macOS clone attempts are
  reserved for files >= 1 MiB. Windows behavior compiles but was not exercised on a Windows host.
