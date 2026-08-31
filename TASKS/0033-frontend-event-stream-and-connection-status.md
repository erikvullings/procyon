# 0033 Frontend SSE stream, reconnection and connection status

Status: done
Priority: high
Owner: unassigned
Agent: unassigned
Area: frontend
Depends on: 0032, 0021

## Context
`file-manager-coding-agent-spec.md` §10 ("the frontend must ...") and §33 step 6.

## Acceptance Criteria
- `frontend/src/api/events/sse-event-stream.ts` implements `EventStream` over `EventSource`,
  maintaining exactly one connection for the app.
- Reconnects with exponential backoff and jitter, capped, resuming from the last received event id.
- Detects stale connections (no keep-alive within a timeout) and forces a reconnect.
- Connection status (`connecting | open | reconnecting | closed`) is exposed in `AppState.connection`
  and shown in a compact indicator in the UI, with text or icon rather than colour alone (§29).
- High-frequency events (`operation.progress`, `directory.delta`) are batched before redraw (§10,
  §13).
- Events from superseded workspace/snapshot revisions are ignored (§10).
- The connection closes on shutdown/logout and does not leak listeners across HMR reloads.
- On resynchronise/gap, affected panes refetch their snapshot rather than applying stale deltas.
- Vitest tests with a fake `EventSource` cover: backoff schedule, stale detection, batching, gap
  handling, revision filtering.

## Implementation Notes
- `HttpFileManagerClient.subscribe()` (0012) delegates here; the Tauri stream (0034) implements the
  same interface so features are transport-agnostic.
- Keep the reconnect policy in a pure, testable function.

## Agent Notes

- 2026-07-31 codex: Implemented the single browser `SseEventStream` with capped exponential
  backoff and jitter, browser-safe last-event-ID resume, observable keep-alive stale detection,
  animation-frame batching for progress/delta events, explicit replay-gap signalling, idempotent
  connection ownership, and listener/timer cleanup. The Axum endpoint now emits named keep-alive
  events (SSE comments are not observable through `EventSource`) and accepts `lastEventId` as the
  browser reconnect equivalent of the `Last-Event-ID` header.
- 2026-07-31 codex: Wired HTTP subscription and transport-neutral disconnect/resynchronise/status
  surfaces through `FileManagerClient`; connected lifecycle teardown to Mithril removal and Vite
  HMR disposal. Connection state uses the required `connecting | open | reconnecting | closed`
  union in `AppState.connection` and appears as accessible text in the application header.
  Directory/workspace events ignore old or foreign revisions, apply contiguous deltas, and refetch
  affected pane snapshots on gaps or discontinuities. There is no logout surface before task 0064;
  its required cleanup boundary is the public `disconnect()` method already used by shutdown/HMR.
- 2026-07-31 codex: Added 10 task-specific tests: 6 fake-`EventSource` tests, 2 app-shell tests,
  and 2 SSE endpoint tests, covering the requested backoff, stale/keep-alive behavior, batching,
  gap handling, revision filtering, accessible status, browser resume, and observable heartbeat.
  Verified the exact affected frontend files (40/40), the focused heartbeat endpoint test (1/1),
  the full frontend package (220/220), strict `tsc --noEmit`, full workspace `pnpm test`, and full
  `pnpm run lint`. `CLAUDE.md` does not exist, so only README was updated.
