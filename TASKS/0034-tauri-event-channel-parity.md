# 0034 Tauri channel event delivery and transport parity

Status: done
Priority: high
Owner: unassigned
Agent: unassigned
Area: desktop
Depends on: 0033, 0015

## Context
`file-manager-coding-agent-spec.md` §11 and §3 rule 9: browser and Tauri transports must provide
equivalent application behaviour.

## Acceptance Criteria
- The Tauri host subscribes to the `EventBus` and forwards envelopes over a Tauri channel/event for:
  directory deltas, operation progress, operation conflicts, filesystem changes and plugin
  notifications (§11).
- `tauri-event-stream.ts` implements `EventStream` with the same status model as SSE; since the
  channel cannot "disconnect" the same way, status transitions are documented and the indicator
  behaves sensibly.
- Payload JSON is byte-identical to the SSE payloads — asserted by a shared fixture test run against
  both transports.
- A parity test suite runs the same frontend scenario (navigate → external file change → delta
  applied) against both the HTTP and Tauri adapters, using the mock where a real host is
  unavailable, and reports explicitly which parts were platform-untested (§35).
- No filesystem or application logic in the command handlers (§3 rule 3).
- Channel subscriptions are released on window close; no task leaks.

## Implementation Notes
- Prefer Tauri channels for high-frequency streams and events for one-off notifications; document
  which is used where.
- Batching happens in the frontend (0033) so both transports share one throttling policy.

## Agent Notes

- 2026-07-31 codex: Implemented the desktop `EventBus` bridge as one ordered Tauri `Channel<String>`
  subscription owned by a dedicated host adapter. The thin `subscribe_events` and
  `unsubscribe_events` commands contain no filesystem/application logic. Both SSE and Tauri now
  call `fm_events::serialize_event_envelope`, making envelope JSON byte-identical by construction;
  replay gaps use a channel control message and trigger the same frontend resynchronisation path.
- 2026-07-31 codex: Completed `TauriEventStream` with idempotent channel setup, typed parsing,
  SSE-equivalent animation-frame batching for `directory.delta`/`operation.progress`, explicit
  teardown, malformed/future-event tolerance, and documented status semantics: `connecting` during
  command setup, `open` until explicit close, `closed` after close/setup failure, and no synthetic
  `reconnecting` state because Tauri exposes no equivalent disconnect signal. One-off notifications
  stay on the ordered channel (instead of a separate Tauri event) to preserve total ordering and
  exact envelope parity.
- 2026-07-31 codex: Subscription tasks are aborted on frontend disconnect and on Tauri window
  destruction. Added 6 frontend tests (4 net-new stream cases plus 2 HTTP/Tauri parameterized
  parity scenarios) and 2 Rust lifecycle/serialization tests. The parity scenarios run the same
  navigate → external file delta → rendered entry flow through SSE and Tauri stream adapters using
  the deterministic mock backend. Verified the exact task-focused frontend files (9/9), affected
  Rust packages, the full frontend package (226/226), strict `tsc --noEmit`, full workspace
  `pnpm test`, and full `pnpm run lint`. `CLAUDE.md` does not exist; README was updated.
- 2026-07-31 codex: Platform-untested: a real WebKit/WKWebView Tauri process and physical window
  close were unavailable in the headless test environment. Tauri's mock runtime smoke test and the
  host adapter task-cancellation tests pass, but live macOS/Windows IPC delivery remains for CI or
  manual desktop verification.
