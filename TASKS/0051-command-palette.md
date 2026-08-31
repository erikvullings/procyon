# 0051 Command palette

Status: done
Priority: high
Owner: unassigned
Agent: unassigned
Area: frontend
Depends on: 0050

## Context
`file-manager-coding-agent-spec.md` §1 (Marta-style command palette), §16 milestone 3, §33 step 8
and §36 item 8.

## Acceptance Criteria
- `Ctrl/Cmd+P` opens a custom palette component (not a Material dialog — §14) listing all available
  actions from the registry.
- Fuzzy filtering over title, id and category, with the shortcut shown per entry and results ranked
  by match quality then recency of use.
- Keyboard-only operation: type to filter, arrows to move, `Enter` to invoke, `Esc` to close; focus
  returns to the previously focused element on close.
- Unavailable actions are either hidden or shown disabled with the reason, never silently
  invocable.
- Actions requiring parameters prompt for them using the `parameter_schema`.
- Opens and filters over a few hundred actions without perceptible lag.
- Accessible: correct combobox/listbox roles, focus trap, screen-reader announcements (§29).
- Vitest tests cover command filtering and ranking (§27) plus keyboard flow.

## Implementation Notes
- Reuse the registry data already loaded for the keybinding dispatcher; do not refetch on every
  open.
- Plugin-contributed actions appear automatically once 0053 lands — no palette changes needed.

## Agent Notes
- 2026-08-01 Codex: Added a custom accessible command palette backed by the already-loaded action registry. It fuzzy-filters title/id/category, ranks by match quality and recency, displays shortcuts and disabled reasons, traps focus, restores prior focus, supports keyboard filtering/navigation/invocation/escape, and renders a typed form for supported object `parameterSchema` properties. Core create-directory remains a local semantic action; other palette invocations use the shared transport-neutral client with active pane/selection context. Verified 4 new focused palette tests and one AppShell keyboard-flow test via Vitest, full frontend Vitest (270 passed, 1 skipped), frontend typecheck, and Rust formatting/clippy. Root Biome lint has two pre-existing `!important` warnings in `frontend/src/features/panes/pane.css`; this task adds none.
