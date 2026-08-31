# 0013 Mock FileManagerClient adapter and fixtures

Status: done
Priority: high
Owner: unassigned
Agent: codex
Area: frontend
Depends on: 0011

## Context
`file-manager-coding-agent-spec.md` §12, §27 (frontend tests use the mock client for deterministic
states) and §28 (mocked 1,000,000-entry datasets must not mount every row).

## Acceptance Criteria
- `frontend/src/api/client/mock-file-manager-client.ts` implements the full `FileManagerClient`
  with deterministic in-memory data.
- Fixtures under `fixtures/mock-responses/` and generators for 1,000 / 10,000 / 100,000 /
  1,000,000-entry directories, generated lazily rather than materialised eagerly where practical.
- Mock supports: nested directories, hidden files, symlink-flagged entries, Unicode names, empty
  directories, unreadable directories (error state), and a slow/loading mode.
- `subscribe()` can emit scripted backend events (directory deltas, operation progress) on demand so
  tests can drive event handling without a server.
- Configurable artificial latency and failure injection for loading/error state testing.
- `pnpm dev:mock` starts the frontend against this adapter with no backend running.

## Implementation Notes
- Keep fixture generation pure and seeded so tests are reproducible.
- The mock is production code used by tests — type it as strictly as the real adapters.

## Agent Notes
- 2026-07-29 codex: Implemented the complete deterministic `MockFileManagerClient` and wired the
  selected client into the frontend bootstrap. Added JSON fixtures for nested, empty, unreadable,
  hidden, symlink, and Unicode cases; pure seeded lazy/random-access generators and paged mock
  locations for 1,000 / 10,000 / 100,000 / 1,000,000 entries; loading states, cancellation-aware
  latency, per-method failures, in-memory operations, and on-demand scripted event delivery.
  Documented `pnpm dev:mock` in README and verified it served the frontend without a backend.
  Verified 12 task-specific tests across `mock-directory-generator.test.ts` and
  `mock-file-manager-client.test.ts`, strict frontend typecheck, production frontend build, and the
  full repository test suite. `pnpm run lint` passes Rust fmt/clippy and all task-touched files pass
  Biome; the repository-wide Biome check still reports pre-existing formatting in
  `scripts/architecture-docs.test.mjs` and `scripts/ci-workflow.test.mjs` (plus an informational
  suggestion in `frontend/vite.config.ts`), which this task intentionally leaves untouched.
