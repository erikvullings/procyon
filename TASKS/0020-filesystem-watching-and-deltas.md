# 0020 Filesystem watching and directory deltas

Status: done
Priority: medium
Owner: unassigned
Agent: unassigned
Area: backend
Depends on: 0019, 0031

## Context
`file-manager-coding-agent-spec.md` §6 (`watch`), §5.4 (`DirectoryDelta`) and §33 steps 4 and 6.
Open directories must reflect external changes without a manual refresh.

## Acceptance Criteria
- The local provider implements `watch` for the directories currently open in a pane.
- Changes are coalesced and debounced, then published as `DirectoryDelta` events
  (`EntriesAdded`, `EntriesUpdated`, `EntriesRemoved`, `Reset`) with the snapshot `revision` they
  apply to.
- A burst of many changes (e.g. extracting 10,000 files) produces batched deltas, not one event per
  file (§28).
- Watch registrations are reference-counted and released when the last pane leaves the directory;
  no watcher leaks after 100 navigations (asserted by a test).
- If the platform watcher drops events or overflows, the service emits `Reset` with a fresh snapshot
  rather than diverging silently.
- Integration tests create/rename/delete files in a temp directory and assert the emitted deltas.

## Implementation Notes
- Use `notify` with a debouncer; document the per-platform caveats (macOS FSEvents coalescing,
  Windows `ReadDirectoryChangesW` buffer overflow) in `docs/architecture/`.
- Deltas carry stable `EntryId`s so the virtualized table can patch rows without a full re-render
  (§13).

## Agent Notes

- 2026-07-30 codex: Added provider-neutral `Changed`/`ResetRequired` invalidations, a bounded
  notify polling watcher with debounce and overflow handling, and filesystem-identity-based stable
  local `EntryId` values.
- 2026-07-30 codex: Added workspace-scoped directory requests and pane-addressed
  `directory.delta` events. `DirectoryService` now shares watches by location with reference
  counting, relists and diffs authoritative snapshots, advances revisions, batches additions,
  and emits a fresh-snapshot `Reset` after dropped events.
- 2026-07-30 codex: Added tests for stable IDs across rename, create/rename/delete event delivery,
  burst coalescing, 10,000-entry delta batching, pagination watch retention, and cleanup after 100
  navigations. Platform behavior and the polling/native-watcher tradeoff are documented in
  `docs/architecture/filesystem-watching.md`.
- 2026-07-30 codex: Validation passed with `pnpm test`, `cargo fmt --all --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`, and the frontend production build.
  `pnpm run lint:frontend` remains blocked only by pre-existing Biome findings in
  `frontend/vite.config.ts`, `scripts/architecture-docs.test.mjs`, and
  `scripts/ci-workflow.test.mjs`.
