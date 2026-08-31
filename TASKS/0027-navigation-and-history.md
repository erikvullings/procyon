# 0027 Directory navigation, parent navigation and history

Status: done
Priority: high
Owner: unassigned
Agent: codex
Area: frontend
Depends on: 0026, 0019

## Context
`file-manager-coding-agent-spec.md` §33 step 5, §36 item 3, and §5.3 (`NavigationHistory` lives in
tab state). This is the first task where the real backend replaces the mock end to end.

## Acceptance Criteria
- Each pane lists a real local directory through `FileManagerClient.listDirectory` / `navigatePane`.
- `Enter` opens the entry under the cursor (directory → navigate, file → open is task 0061).
- `Backspace` navigates to the parent; at the filesystem root it is a no-op, not an error.
- Back/forward history per tab, with keyboard and mouse-button support where available.
- Rapid navigation cancels the superseded request via `AbortSignal`; a late response never
  overwrites a newer view (§5.4) — covered by a test that resolves responses out of order.
- Loading state appears within one frame; the first page renders without waiting for all metadata
  (§28).
- Error states (permission denied, not found, disconnected) render in-pane with a retry affordance
  and a user-readable message from the error DTO.
- Paging: scrolling to the end of a partially loaded directory requests the next page via
  `continuation_token`.
- Vitest tests with the mock client cover: navigate, parent, history back/forward, out-of-order
  responses, error rendering.

## Implementation Notes
- Navigation logic belongs in `features/navigation/`, not in the table or pane components (§35).
- Keep pane → backend request correlation by `requestId` so stale snapshots can be dropped.

## Agent Notes
- 2026-07-31 codex: Added a transport-neutral controller under `features/navigation/` that loads
  each active tab's real directory, publishes loading synchronously, opens directories, traverses
  parents, retries readable errors, and appends continuation-token pages. Requests are cancelled
  per pane and both active-request identity and backend `requestId` must match before a snapshot is
  rendered, including an out-of-order response regression test.
- 2026-07-31 codex: Added Enter, Backspace, Alt+Left/Right, auxiliary mouse-button, and visible
  back/forward/parent controls to the presentation-only pane. Back and forward targets are resolved
  from authoritative backend history; the shared Rust domain/DTO/OpenAPI contract and mock adapter
  now omit targets for those modes while push/refresh still require one.
- 2026-07-31 codex: Added 12 task-specific tests across Vitest and Rust for loading/navigation,
  parent/root behaviour, backend-resolved back/forward history, cancellation and stale responses,
  paging, retryable errors, pane input, scroll paging, DTO compatibility, and empty history. Full
  repository lint, all Rust/frontend/script tests, frontend typecheck, and the production frontend
  build pass. Shared Axum and Tauri application paths compile and test; Windows-specific auxiliary
  mouse behaviour was not manually exercised on this macOS development host.
