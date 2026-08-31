# 0021 Frontend application state model

Status: done
Priority: high
Owner: unassigned
Agent: unassigned
Area: frontend
Depends on: 0011

## Context
`file-manager-coding-agent-spec.md` §13: a small, explicit state model rather than a large generic
state-management framework (§35 forbids introducing one without demonstrated need).

## Acceptance Criteria
- `frontend/src/state/` defines `AppState` with `runtime`, `workspace`, `operations`, `plugins`,
  `notifications` and `connection` slices, all strongly typed and readonly.
- Updates are patch-based and immutable; major snapshots are replaced wholesale rather than mutated
  (§13).
- High-frequency updates (operation progress, directory deltas) are batched before redraw, with a
  single scheduling primitive used by every producer (§13, §28).
- Entries are keyed by stable `EntryId` throughout (§13).
- Application logic lives in state/actions modules, not in Mithril components (§35).
- Vitest tests cover: patch application, immutability of prior snapshots, batching (N updates in one
  frame produce one redraw), and slice reducers in isolation.

## Implementation Notes
- Use Meiosis (`m.stream` + Mergerino) per the local `meiosis` skill; record the choice in ADR 7
  (task 0005).
- Avoid a global redraw for every file-list event (§13) — prefer targeted subscriptions in the
  table component (0024).

## Agent Notes
- 2026-07-30 codex: Implemented the readonly, strongly typed `AppState` and its `runtime`,
  `workspace`, `operations`, `plugins`, `notifications`, and `connection` slices under
  `frontend/src/state/`. The Meiosis loop uses `mithril/stream` (the supported Mithril 2.x stream
  module) with immutable Mergerino patches, one injected animation-frame scheduler, and targeted
  selector subscriptions. Workspace and directory snapshots are replaced wholesale; directory
  projections normalize entries into stable-`EntryId` maps. Pure slice reducers and typed actions
  keep application logic outside Mithril components.
- 2026-07-30 codex: Added 7 task-specific Vitest tests across `store.test.ts`,
  `reducers.test.ts`, and `actions.test.ts`, covering immutable patch application, preservation of
  prior snapshots, N-to-one frame batching/redraw, targeted subscriptions, wholesale snapshots,
  realistically interleaved directory deltas, and isolated slice reducers/actions. Verified those
  exact files 7/7, the full frontend suite 78/78, strict `tsc --noEmit`, and the production Vite
  build. `pnpm run lint:frontend` still reports only pre-existing formatting failures in
  `scripts/architecture-docs.test.mjs` and `scripts/ci-workflow.test.mjs` plus an informational
  suggestion in `frontend/vite.config.ts`; `pnpm exec biome check frontend/src/state
  frontend/package.json` is clean.
