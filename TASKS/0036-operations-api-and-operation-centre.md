# 0036 Operations API and operation centre UI

Status: done
Priority: high
Owner: unassigned
Agent: unassigned
Area: backend
Depends on: 0035, 0033

## Context
`file-manager-coding-agent-spec.md` §8 (operation endpoints), §14 (operation centre in the main
window) and §36 item 5.

## Acceptance Criteria
- REST endpoints with stable operation ids:
  - `GET /api/v1/operations` → `listOperations`
  - `POST /api/v1/operations` → `startOperation`
  - `GET /api/v1/operations/{operationId}` → `getOperation`
  - `POST /api/v1/operations/{operationId}/cancel` → `cancelOperation`
  - `POST /api/v1/operations/{operationId}/pause` → `pauseOperation`
  - `POST /api/v1/operations/{operationId}/resume` → `resumeOperation`
  - `POST /api/v1/operations/{operationId}/resolve-conflict` → `resolveOperationConflict`
- `startOperation` accepts the semantic request shape from §7 (`type`, `sources`, `destination`,
  `conflictPolicy`) and honours an idempotency key so a retried request does not start a second job
  (§8).
- Equivalent Tauri commands exist (§11) and share the same service methods.
- `FileManagerClient` gains `startOperation`, `cancelOperation`, `resolveConflict`, `listOperations`
  across all three adapters.
- Operation centre UI in `features/operations/`: queued/running/paused/completed/failed operations
  with per-operation progress, rate, current entry, and cancel/pause/resume controls.
- Progress updates arrive via events and are batched; the UI does not poll.
- Completed operations remain visible with their result until dismissed; failures show the
  user-readable message plus a details expander.
- Vitest tests for progress reducer and operation centre states; Rust integration test starts a
  no-op operation and observes the full event sequence.

## Implementation Notes
- The frontend issues semantic operations only and never enumerates or copies files itself
  (§7, §35).
- Reserve the conflict endpoint's DTO now; the dialog lands in 0045.

## Agent Notes

- 2026-07-31 codex: Added shared operation DTOs and `FileManagerService` methods; seven stable
  Axum operation endpoints; atomic `Idempotency-Key` retry deduplication; matching Tauri commands;
  and generated OpenAPI/Orval artifacts. The reserved conflict-decision DTO supports one decision,
  applying it to similar conflicts, or cancelling the operation. A no-op executor exercises the
  API boundary until concrete operation kinds land in tasks 0037–0044.
- 2026-07-31 codex: Completed the HTTP, Tauri, and deterministic mock `FileManagerClient` operation
  surfaces and replaced the shell placeholder with an event-driven operation centre. Progress is
  reduced in animation-frame batches without polling; queued/running/paused/completed/failed jobs
  show progress, rate, current entry, lifecycle controls, retained results, dismissal, and
  expandable failure details.
- 2026-07-31 codex: Added 2 task-specific Rust integration tests in `operation_routes.rs`, verified
  directly with `cargo test -p fm-server --test operation_routes`, covering stable operation IDs,
  idempotent retry, and the exact no-op lifecycle event sequence. Added 3 task-specific Vitest
  tests in `operation-centre.test.ts`, verified directly with `vitest run`, covering realistically
  interleaved progress reduction and the required operation-centre states. Also verified all
  affected Rust package suites, the Tauri mock-runtime suite, the complete 233-test frontend suite,
  strict `tsc --noEmit`, full workspace Clippy/formatting and Biome lint, and regenerated API
  freshness. Tauri commands compile and the desktop mock runtime passes on macOS, but the new
  operation commands were not manually invoked in a packaged desktop application. `CLAUDE.md`
  does not exist, so only README documentation was updated.
