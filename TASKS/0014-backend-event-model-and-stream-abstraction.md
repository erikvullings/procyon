# 0014 Typed backend event model and event-stream abstraction

Status: done
Priority: high
Owner: unassigned
Agent: codex
Area: frontend
Depends on: 0011

## Context
`file-manager-coding-agent-spec.md` §10 and §33 step 3. Both transports must deliver the same typed
events, so the event model and the `EventStream` abstraction are defined before either SSE (0032) or
Tauri channels (0034) exist.

## Acceptance Criteria
- `frontend/src/api/events/event-stream.ts` defines an `EventStream` interface with
  `connect()`, `close()`, a status observable (`connecting | open | reconnecting | closed`) and a
  listener registry.
- `BackendEvent` is a discriminated union over the named events from §10:
  `runtime.ready`, `workspace.updated`, `directory.snapshot`, `directory.delta`,
  `operation.created`, `operation.progress`, `operation.stateChanged`, `operation.conflict`,
  `operation.completed`, `operation.failed`, `plugin.changed`, `notification.created`.
- Events are wrapped in the `EventEnvelope { eventId, timestamp, workspaceId?, payload }` shape.
- The Rust counterpart (`fm-events` envelope + payload enum) serializes to exactly this JSON; a
  cross-checked test fixture (Rust-generated JSON consumed by a Vitest test) proves parity.
- Unknown/future event types are ignored without throwing, and are logged once in development.
- Vitest tests cover envelope parsing, unknown-event tolerance and listener dispatch.

## Implementation Notes
- Event payload types should be generated or derived from the OpenAPI schemas where possible so they
  stay in sync; if `utoipa` cannot express the union cleanly, document the manual mapping.
- Do not use events for request/response semantics (§10).

## Agent Notes
- 2026-07-29 codex: Implemented the typed backend event contract and transport-neutral frontend
  stream abstraction.
  - `fm-events` now defines the generic camelCase `EventEnvelope`, all 12 named
    `BackendEventPayload` variants, and strongly typed manual wire projections for workspace,
    directory, operation, plugin, conflict, and notification data. These projections mirror the
    OpenAPI/frontend models without depending on the peer-layer `fm-transport-dto` crate.
  - `frontend/src/models/events.ts` now exposes the matching discriminated union.
    `frontend/src/api/events/event-stream.ts` defines `EventStream`, its four-state status
    observable, a listener registry, and tolerant parsing. Unknown future types are ignored and
    logged once in development.
  - Added a checked-in Rust serialization fixture consumed directly by Vitest.
  - Verified 3 task-specific Rust tests with `cargo test -p fm-events` and 4 task-specific Vitest
    tests with `vitest run src/api/events/event-stream.test.ts`; `pnpm test` passes the complete
    workspace suite, `pnpm --dir frontend run typecheck` is clean, Rust fmt/clippy are clean, and
    scoped Biome checks are clean.
  - Known unrelated lint baseline: root `pnpm run lint` still reports pre-existing formatting in
    `scripts/architecture-docs.test.mjs` and `scripts/ci-workflow.test.mjs`, plus an informational
    suggestion in `frontend/vite.config.ts`; this task leaves those files untouched.
