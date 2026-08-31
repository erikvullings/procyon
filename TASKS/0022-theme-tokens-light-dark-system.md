# 0022 CSS variable themes: light, dark and follow-system

Status: done
Priority: medium
Owner: unassigned
Agent: unassigned
Area: frontend
Depends on: 0002

## Context
`file-manager-coding-agent-spec.md` §14 ("Themes" and "Visual direction"). The UI must be polished,
compact, keyboard-first, information-dense and consistent across macOS and Windows.

## Acceptance Criteria
- `frontend/src/themes/` defines the design tokens listed in §14: `--fm-background`, `--fm-surface`,
  `--fm-surface-elevated`, `--fm-text`, `--fm-text-muted`, `--fm-border`, `--fm-accent`,
  `--fm-selection`, `--fm-selection-inactive`, `--fm-hover`, `--fm-error`, `--fm-warning`,
  `--fm-success`, `--fm-row-height`, `--fm-font-family`, `--fm-font-size`, `--fm-radius`,
  `--fm-shadow`.
- Light and dark themes plus a follow-system mode driven by `prefers-color-scheme`, switchable at
  runtime without reload.
- `mithril-materialized` is themed from the same tokens so dialogs and forms match the panes.
- No component hard-codes a colour (§14); a test or lint rule greps the component sources for hex
  colours and fails on new ones.
- Contrast of text on surface and of selection states meets WCAG AA (§29); documented in
  `docs/architecture/theming.md`.
- `prefers-reduced-motion` disables transitions (§29).

## Implementation Notes
- Distinct `--fm-selection` vs `--fm-selection-inactive` matters: the inactive pane must show its
  selection dimmed, not hidden.
- Selection and cursor must be distinguishable without colour alone (§29) — plan for a border or
  marker as well.

## Agent Notes
- 2026-07-30 codex: Implemented the complete `--fm-*` token palette in
  `frontend/src/themes/theme.css`, with explicit light/dark themes and attribute-free follow-system
  mode compatible with mithril-materialized's existing `ThemeManager`. Materialized `--mm-*`
  variables derive from the same tokens. Added active/inactive selection styling, distinct
  non-colour selection/cursor markers, and a reduced-motion override.
- 2026-07-30 codex: Added 8 task-specific tests: 6 stylesheet/contrast/marker tests and 1
  source-colour guard in the two new theme test files, plus 1 runtime-switching AppShell test.
  Verified those tests explicitly; the full frontend suite passes (86 tests), `pnpm test` passes,
  frontend typecheck and production build pass, and Biome reports no warnings or errors in the
  touched frontend files. Documented the runtime contract and measured WCAG contrast ratios in
  `docs/architecture/theming.md`.
- 2026-07-30 codex: Repository-wide `pnpm run lint` remains blocked by pre-existing Biome findings
  in `frontend/vite.config.ts`, `scripts/architecture-docs.test.mjs`, and
  `scripts/ci-workflow.test.mjs`; these files were outside this task and were not changed.
