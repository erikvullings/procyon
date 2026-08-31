# 0023 Development-only mithril-inspector integration

Status: done
Priority: low
Owner: unassigned
Agent: unassigned
Area: frontend
Depends on: 0002

## Context
`file-manager-coding-agent-spec.md` §2.1 requires `mithril-inspector` in development builds to help
coding agents debug component behaviour, and forbids it from reaching production.

## Acceptance Criteria
- `@mithril-inspector/vite` is configured in `frontend/vite.config.ts` with the `code` editor,
  `full` mode and system-themed UI.
- The Vite plugin's development-only default is preserved: `includeInProduction` is not enabled.
- A production build contains no reference to the inspector — asserted by a test that greps `dist/`.
- The inspector allows: inspecting the component tree, viewing component attrs and local state,
  selecting rendered elements, and tracing an element to its source component where supported.
- No core application behaviour depends on the inspector being present.
- Documented in the README: how to open it and what it is useful for.

## Implementation Notes
- There is no `mithril-inspector` package on npm (checked in task 0002). It ships as a scoped set:
  `@mithril-inspector/vite` (the bundler plugin), plus `runtime`, `overlay`, `server`, `protocol`,
  `transform` and builds for rollup/esbuild — all at 0.3.2. Start from the Vite plugin.
- Use the Vite plugin's zero-application-code integration. It injects the overlay, owns HMR
  invalidation and prevents duplicate overlay initialisation.
- Do not add an application-side loader or make application startup depend on inspector globals.

## Agent Notes
- 2026-07-30 codex: Configured `@mithril-inspector/vite` 0.3.2 with VS Code, full inspection mode
  and the system UI theme. The plugin owns development-only activation, overlay injection and HMR
  initialisation; application startup has no inspector dependency. Updated the task contract to
  match the published Vite plugin API and documented opening and using the inspector in `README.md`.
- 2026-07-30 codex: Added 2 integration tests in `config/mithril-inspector.test.ts`: development
  config registration and a real production-build scan proving inspector code and markers are
  absent. Verified those 2 tests directly, all 88 frontend tests, clean `tsc --noEmit`, a production
  Vite build, and a live development overlay in Chrome with Components, Elements, History,
  Settings and element-selection controls. Scoped Biome checks pass; the repository-wide
  `lint:frontend` remains blocked by pre-existing formatting errors in
  `scripts/architecture-docs.test.mjs` and `scripts/ci-workflow.test.mjs`.
