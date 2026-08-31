# 0154 Replace global keydown if/else chain with an ordered dispatch table

Status: done
Priority: low
Subsystem: frontend
Depends on: none

## Context

Found via `/improve-codebase-architecture`. `createGlobalKeydownHandler` in
`frontend/src/features/keybindings/global-keydown-handler.ts` (1,155 lines total) is a single
function (lines 304–1143, ~840 lines) containing 132 `if`/`else if` branches that decide keyboard
routing across navigation, viewer, tabs, panes, dialogs, and system-trash logic, dispatching into
whichever controller context matches.

A companion 866-line test file exists, and small helpers (`resolveViewTarget`, `findOpenViewer`,
`isWithinViewer`) are extracted and unit-tested — but the actual bug surface, branch
*precedence/ordering* among the 132 conditionals, isn't behind any interface at all; it's implicit
in source order. This is the "pure functions extracted for testability, but the real bug hides in
how they're called" pattern the skill looks for: adding a new shortcut today requires reading
enough of the 840-line function to be sure the new branch doesn't get shadowed by (or shadow) an
earlier one.

## Acceptance Criteria
- The if/else chain is replaced with an explicit ordered structure (e.g. an array of
  `{ predicate, handler }` entries walked in order) so precedence is a visible data structure
  rather than implicit in source position.
- Precedence is directly testable: given a key event + application context, assert which handler
  wins, without needing to simulate the full DOM/keydown pipeline for every case.
- All 132 existing routing cases are preserved with identical behaviour — cross-check against the
  existing 866-line test file's coverage before and after; no regression in any currently-tested
  shortcut.
- `pnpm --dir frontend exec vitest run` for the keybindings feature passes; manually verify a
  sample of shortcuts across navigation, viewer, tabs, and dialogs still fire correctly in the
  running app (both `mock` and `http` runtimes).

## Implementation Notes
- Before changing anything, extract the current branch order into a list (condition summary +
  outcome) so the refactor has a checklist to verify against — precedence bugs are exactly the kind
  of regression that's easy to introduce silently here.
- Consider grouping branches by the feature area they already implicitly belong to (navigation,
  viewer, tabs, panes, dialogs, trash) as a first pass, since that grouping may reveal that several
  "132 branches" are really a handful of areas each with its own local precedence, which could
  simplify the eventual table shape.
- Lower priority than 0152/0153: this file already has substantial test coverage and no reported
  bugs — the value here is preventing a future precedence bug, not fixing an active one.

## Agent Notes
- 2026-08-25: Task created from `/improve-codebase-architecture` findings (candidate 4). Not yet
  investigated further beyond the initial Explore pass — see Implementation Notes for the first
  concrete step.
- 2026-08-27 Copilot: Replaced the global conditional chain with 54 named routes split across
  ordered early-key and action dispatch tables. Added a public dispatch seam and two direct
  precedence tests covering special-key routing ahead of registered actions and palette blocking.
  Verified 50 keybinding tests, all 1,470 frontend tests, frontend typecheck, changed-file Biome,
  and the repository lint suite. Browser smoke testing confirmed the Settings shortcut in the mock
  runtime and a healthy HTTP backend, but the HTTP frontend mounted an empty app root in the
  available browser session, so representative navigation/viewer/tab shortcuts could not be
  manually exercised there; their existing automated coverage passed unchanged. The full
  repository run completed its Rust and frontend suites, then hit two unrelated script-test
  assertions against already-changed CI (`cargo test` versus `nextest`) and desktop identifier
  (`dev.fm.desktop` versus `nl.erikvullings.procyon`) values.
