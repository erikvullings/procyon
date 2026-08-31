# 0081 Workspace events over the shared event bus

Status: done
Priority: medium
Owner: unassigned
Agent: unassigned
Area: backend
Depends on: 0080, 0031

## Context
`file-manager-coding-agent-spec.md` §5.3.11 and §10. Workspace events describe configuration
changes only; directory contents keep arriving through the separate snapshot/delta events (0019,
0020, 0032).

## Acceptance Criteria
- Every `WorkspaceCommand` from 0080 publishes exactly one of the 12 named events from §5.3.11
  (`workspace.created`, `workspace.renamed`, `workspace.opened`, `workspace.closed`,
  `workspace.deleted`, `workspace.layoutChanged`, `workspace.activePaneChanged`,
  `workspace.tabAdded`, `workspace.tabClosed`, `workspace.tabActivated`, `workspace.tabNavigated`,
  `workspace.tabViewChanged`) through the `EventBus` (0031), each envelope carrying the workspace
  id, the new `revision`, and a mutation-specific payload.
- `workspace.opened`/`workspace.closed` are also published on the lifecycle transitions from §5.3.7
  (startup, switching workspaces), not only in response to an explicit command.
- Payload shapes are covered by a fixture test shared with the frontend event union (mirroring
  0014's approach), so the SSE (0032) and Tauri (0034) transports need no workspace-specific changes
  once they exist.
- Directory contents are never embedded in a workspace event (§5.3.11) — asserted by a test that a
  directory-snapshot-shaped payload cannot type-check as a workspace event payload.
- Unit tests: one event per command/lifecycle transition, with the correct revision and payload
  shape.

## Implementation Notes
- This task only wires publication into `WorkspaceService`; it does not implement SSE or Tauri
  transport — 0032/0034 already carry any `EventEnvelope` generically.

## Agent Notes
- Task 0031 (`EventBus`) does not exist yet. Per user decision, this task extends the existing
  `WorkspaceCommandPublisher` seam from 0080 instead of blocking on 0031: `WorkspaceService`
  constructs the 12 named `BackendEventPayload::Workspace*` variants and publishes them through
  the injected publisher trait. Wiring the publisher into a real `EventBus`/transport is deferred
  to 0031; the trait signature (`publish(workspace_id, payload)`) is designed so that swap-in
  requires no further changes to `WorkspaceService`.
- Payloads are intentionally minimal/focused (revision + the single changed field, e.g. `name`,
  `layout`, `paneId`/`tabId`, `location`, `view`) rather than embedding full workspace/tab
  projections, replacing the old lossy `WorkspacePayload`/`TabStatePayload`/`DirectoryViewStatePayload`
  placeholder types from 0014. Shared JSON fixtures under `fixtures/events/` (one per event type)
  are asserted identical from both the Rust (`fm-events`) and TypeScript (`frontend/src/models/events.ts`,
  `event-stream.test.ts`) sides.
- `workspace.opened`/`workspace.closed` are emitted both for explicit lifecycle calls (§5.3.7:
  `WorkspaceService::start`, `create_default`) and for `open()` (§5.3.12, closes the previously
  active workspace and opens the requested one). The fuller "switching workspaces" steps from
  §5.3.7 (debounced view-state flush, keeping in-flight operations alive across the switch,
  closing directory subscriptions, ordered tab reloading) are not implemented here — they belong
  to their owning systems (0020 filesystem watching, 0035/0036 pane/tab loading) and are out of
  scope for this event-plumbing task.
- Found and fixed a pre-existing latent bug while adding test coverage: `BackendEventPayload`'s
  `#[serde(tag = "type")]` had no `rename_all`, so flat fields on existing variants (e.g.
  `OperationStateChanged.operation_id`, `OperationFailed.operation_id`) were serializing as
  snake_case instead of the camelCase the frontend already expected. Fixed via serde's
  `rename_all_fields = "camelCase"` container attribute (note: `rename_all` on an enum only
  affects variant/tag names, not fields inside struct-like variants — `rename_all_fields`,
  stable since serde 1.0.166, is needed for the latter). Applied the same fix to the pre-existing
  `WorkspaceLayoutPayload` enum (its `pane_id` field). Deliberately left the same latent bug
  unfixed in the unrelated `DirectoryDeltaPayload` enum — no fixture/test currently exercises it,
  and fixing it is out of scope for this task; flagged here as a known follow-up.
- Verification: `cargo test --workspace` all green (fm-events: 4 tests; fm-application: 68 tests,
  including 5 new/rewritten publisher-recording tests covering create/create_default/delete/open/
  apply_command). `cargo clippy --workspace --all-targets -- -D warnings` clean. `cargo fmt --all
  -- --check` clean. Frontend: `tsc --noEmit` clean; `vitest run` 70/70 tests pass across the
  workspace, including 17 in `event-stream.test.ts` (12 new fixture round-trip tests plus a
  `@ts-expect-error` type-safety test proving a `directory.snapshot` payload cannot satisfy a
  workspace event payload type). `biome check` clean on all files touched by this task.
  `pnpm run api:check` was not run to completion as a pass/fail gate mid-task since it diffs the
  full working tree via `git diff --exit-code`; confirmed manually that `api:export`/`api:generate`
  produced no changes to `frontend/openapi/openapi.json` or the generated Orval client, i.e. this
  task's changes do not affect the OpenAPI-generated surface.
