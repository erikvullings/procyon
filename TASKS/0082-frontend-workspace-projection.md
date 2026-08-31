# 0082 Frontend WorkspaceProjection, state slice and command dispatch

Status: done
Priority: high
Owner: unassigned
Agent: unassigned
Area: frontend
Depends on: 0080, 0021, 0011

## Context
`file-manager-coding-agent-spec.md` §5.3.13, §5.3.10 and §13. The frontend must mutate workspaces
only through the semantic commands from 0080, never by replacing arbitrary JSON (§5.3.9), and must
keep a normalized projection rather than copying directory entries into workspace state.

## Acceptance Criteria
- `WorkspaceProjection`/`PaneProjection`/`TabProjection` types in `frontend/src/models/workspace.ts`
  exactly per §5.3.13 — normalized (`paneOrder`/`panesById`, `tabOrder`/`tabsById`), replacing the
  current ad hoc `Workspace` shape from 0011 wherever it no longer matches. Flag any breaking change
  to 0011's existing consumers explicitly in Agent Notes rather than silently adjusting them.
- `AppState.workspace` (0021) is sourced from the projection; directory snapshots are stored
  separately, keyed by tab or directory-session id, so a workspace mutation never replaces or
  copies a large entry array (§5.3.13).
- `WorkspaceViewState` (frontend-only cursor/selection/dialog/drag state, §5.3.3) lives in its own
  state slice, is never sent to the backend and is never derived from or merged into the
  `WorkspaceProjection`.
- `FileManagerClient` (0011) gains the workspace command surface — `listWorkspaces`,
  `createWorkspace`, `renameWorkspace`, `deleteWorkspace`, `openWorkspace`,
  `dispatchWorkspaceCommand` (or an equivalent split into per-command methods) — implemented across
  all three adapters (`Http`/`Tauri`/`Mock`), mirroring how task 0036 extends the interface for
  operations.
- A stale-revision (`workspaceRevisionConflict`) response reloads the latest projection and only
  retries the mutation when it is safely idempotent (§5.3.10); a non-idempotent stale mutation
  surfaces to the user instead of silently retrying.
- Vitest tests: projection normalization from a fixture matching §5.3.15's example JSON,
  revision-conflict reload/no-silent-retry behaviour, and a test asserting a workspace mutation
  leaves previously stored directory entries untouched.

## Implementation Notes
- Keep workspace command dispatch in `features/workspace/`, not inside pane/table components (§35).
- This extends the `FileManagerClient` interface itself, the same pattern task 0036 uses for
  operations — update the `NotImplementedError` owning-task references for these new methods in the
  HTTP/mock/Tauri adapters if they were stubbed with a stale task number before this task lands.

## Agent Notes
- 2026-07-30 codex: Implemented the exact normalized `WorkspaceProjection`/`PaneProjection`/
  `TabProjection` model from §5.3.13 and a DTO normalizer covered by the persisted-workspace example
  fixture. `AppState.workspace.current` now holds that projection; normalized directory entries are
  retained separately by opaque request/session id, and projection mutations preserve the directory
  cache by identity. Added an independent `workspaceView` slice plus typed actions for frontend-only
  cursor, selection, dialog and drag state.
- 2026-07-30 codex: Extended `FileManagerClient` and the HTTP, Tauri and deterministic mock adapters
  with list/create/get/rename/delete/open/semantic-command workspace operations. Command dispatch
  reloads the latest projection on `workspaceRevisionConflict`; state-setting commands retry at the
  new revision, `navigateTab` retries only for `refresh`, and `addTab`/`closeTab` surface the conflict
  after reload without replay.
- 2026-07-30 codex: This deliberately breaks task 0011's provisional consumers: the former
  `Workspace`/`PaneState`/`TabState` shape and its persisted `DirectoryViewState` selection/cursor
  fields were replaced, and the layout discriminator now uses the generated backend's `axis` field
  instead of the old ad hoc `direction`. Existing state and all three adapters were migrated; no
  Mithril component consumed the removed shape.
- 2026-07-30 codex: Added 7 task-specific Vitest cases across `workspace.test.ts`,
  `dispatch-workspace-command.test.ts`, `reducers.test.ts`, `http-file-manager-client.test.ts`, and
  `mock-file-manager-client.test.ts`; the Tauri adapter's former TBD test was replaced with a real
  projection test. Verified the exact affected test files, strict `tsc --noEmit`, the production
  Vite build, all 95 frontend tests, and the repository-wide `pnpm test` (Rust, frontend and script
  suites). Task-touched files pass Biome. `pnpm run lint` still fails only on pre-existing formatting
  in `scripts/architecture-docs.test.mjs` and `scripts/ci-workflow.test.mjs`; the existing
  `frontend/vite.config.ts` literal-key suggestion remains informational.
