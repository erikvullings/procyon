# 0048 In-application clipboard copy / cut / paste

Status: done
Priority: medium
Owner: unassigned
Agent: Codex
Area: frontend
Depends on: 0047

## Context
`file-manager-coding-agent-spec.md` §16 milestone 2 — clipboard-based copy/cut/paste within the
application.

## Acceptance Criteria
- `Ctrl/Cmd+C`, `Ctrl/Cmd+X`, `Ctrl/Cmd+V` copy, cut and paste the selection between panes and tabs.
- The clipboard holds `Location`s plus the intended mode (copy or move); paste starts the
  corresponding operation through the engine — the frontend never moves files itself (§35).
- Cut entries are visually dimmed until pasted or the cut is cleared; pasting a cut clears it.
- Pasting into an invalid target (source's own subtree, read-only location, missing directory) is
  rejected before starting an operation, with a clear message.
- Stale clipboard entries (source deleted or moved since the copy) produce a per-entry warning
  instead of failing the whole paste.
- Interop with the system clipboard is capability-gated (§21 `clipboard`): where supported, copying
  also places file references/paths on the system clipboard; where not, the in-app clipboard still
  works.
- Vitest tests cover clipboard state transitions and target validation.

## Implementation Notes
- Keep the clipboard in `AppState`, not in a component.
- System-clipboard file references differ per platform; implement them via the platform adapter
  (0058) rather than here.

## Agent Notes
- 2026-08-01 — Added AppState-owned in-app clipboard state and Ctrl/Cmd copy, cut and paste
  shortcuts. Directory snapshots now expose destination writability, allowing the UI to reject
  unavailable, read-only and source-subtree targets before starting the corresponding operation.
  Copy and move operations accept multiple sources; stale copy/move sources are reported as
  per-entry warnings while remaining sources proceed. System clipboard file-reference integration
  remains intentionally deferred to the platform adapter in task 0058; the in-app clipboard works
  in every current host. Verified with affected Vitest suites, `pnpm --dir frontend typecheck`,
  `cargo test -p fm-application --test copy_file_operation`, and affected Rust crate tests.
