# 0094 Tabler icon subset for the workspace toolbar

Status: done
Priority: low
Owner: unassigned
Agent: copilot
Area: frontend
Depends on: none

## Context

QA request (live round 3): "Create a new task for the tabler/icons (subset)
and implement it." The workspace toolbar (Back/Forward/Parent directory/Find
files/Command palette/Workspace switcher/Settings) used plain Unicode glyphs
(←, →, ↑, ×) and text-only buttons; the request is to give it real icons
using Tabler Icons (https://tabler.io/icons), matching the existing "vendor a
minimal SVG subset rather than a full icon-font dependency" pattern already
used for the Catppuccin directory-entry icon theme (task 0092,
`frontend/src/themes/catppuccin-icons.ts`).

## Acceptance Criteria

- A small, curated subset of Tabler Icons (MIT licensed) is vendored as
  inline SVG icon components, following the existing `IconAttrs` /
  `trustedIcon` pattern from `frontend/src/components/icons.ts` and
  `frontend/src/themes/catppuccin-icons.ts`.
- The workspace toolbar's Back, Forward, Parent directory, Find files,
  Command palette, Workspace switcher, and Settings controls use these
  icons; the settings/workspace-switcher/close (×) affordances render icons
  instead of bare glyphs.
- Existing tests (`app-shell.test.ts`) continue to pass unchanged (icons are
  `aria-hidden`, so they don't affect `aria-label`/`textContent` assertions).
- `tsc --noEmit`, Biome, and the full Vitest suite stay clean.

## Implementation Notes

- New file: `frontend/src/components/tabler-icons.ts` — vendors
  `arrowLeftIcon`, `arrowRightIcon`, `cornerLeftUpIcon`, `searchIcon`,
  `commandIcon`, `settingsIcon`, `layoutGridIcon`, `closeIcon` from
  https://github.com/tabler/tabler-icons `icons/outline/*.svg` sources
  (MIT, Copyright (c) 2020-2024 Paweł Kuna), 24x24 stroke-based viewBox
  (`stroke="currentColor"`, `fill="none"`), reproduced verbatim as trusted
  inline markup rather than an npm dependency, since only a handful of
  glyphs are needed.
- Wired into `frontend/src/app/app-shell.ts`'s toolbar (`.fm-workspace-toolbar`
  block) and the settings/workspace-switcher close buttons.
- `frontend/src/themes/theme.css`: `.fm-navigation-controls button`,
  `.fm-workspace-toolbar > button`, `.fm-settings-button`,
  `.fm-settings-editor button` now use `display: inline-flex; align-items:
  center; gap: 0.35rem;` so icon+label buttons line up correctly.

## Agent Notes

- Implemented and verified: `tsc --noEmit` clean, Biome clean, full Vitest
  suite (433 tests) green. No `@tabler/icons` npm package was added -
  vendoring a handful of raw SVGs matches the codebase's existing
  established convention and avoids a new runtime dependency for ~8 icons.
- If more icons are needed later (e.g. for context menus, function-key bar),
  extend `tabler-icons.ts` with the same `trustedStrokeIcon` helper rather
  than introducing the npm package, unless the vendored subset grows large
  enough that maintaining it by hand becomes a burden.
