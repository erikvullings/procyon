# 0223 Workspace chrome and splitter polish

Status: done
Priority: high
Subsystem: frontend
Depends on: 0025, 0026, 0045, 0094, 0222

## Context

The compact workspace chrome has several alignment and precision defects after the Materialized 4
integration: active-pane emphasis redundantly recolours the center divider, breadcrumb text is not
vertically centered, and the New tab, View, and Favourites actions do not share one aligned button
geometry. The conflict resolver also remains unnecessarily narrow on wide windows. Finally, the
draggable pane splitter gives no visible percentage feedback, making an exact 50/50 split difficult.

## Acceptance Criteria

- Active-pane state no longer changes the center divider colour; cursor and pane chrome continue to
  communicate command focus.
- Coarse-pointer toolbar controls have 44px hit targets while retaining existing actions and
  tooltip labels.
- Breadcrumb text and the New tab, View, and Favourites controls are vertically aligned, with equal
  button widths and consistent icon geometry.
- The conflict-resolution dialog uses more horizontal space on wide windows while remaining bounded
  and responsive on narrow screens without showing a spurious vertical scrollbar.
- Dragging the pane splitter exposes an accessible live percentage and makes 50/50 easy to reach.
- Breadcrumb labels and the editable path sit optically centered within the compact row.
- Clicking away from an edited breadcrumb cancels the draft without navigating.
- Enter is the only way to submit an edited path; failed paths retain the current directory and
  show a localized warning toast.
- Focused frontend tests, typecheck, and lint pass.

## Implementation Notes

- Preserve keyboard-first density and all visible metadata/function-key actions.
- Reuse existing Tabler icons and Materialized dialog behavior.
- Keep split persistence through the existing debounced `WorkspaceLayout` update path.

## Agent Notes

- 2026-09-13 Copilot: Started as five ordered passes requested by the user: divider/touch chrome,
  breadcrumb actions, conflict width, splitter feedback, then integrated polish.
- 2026-09-13 Copilot: Removed redundant active-pane divider colouring; added 44px coarse-pointer
  toolbar targets with local horizontal scrolling; centered breadcrumb content and normalized the
  New tab, View, and Favourites controls; widened conflict resolution to a responsive 44rem; and
  added live split percentages, center snapping, ARIA values, and 1%/5% keyboard resizing. Verified
  at 1440x900 and 390x844, with no page-level horizontal overflow. Frontend typecheck, repository
  lint, and all 2,127 frontend tests pass (the suite was run with one worker because its filesystem
  boundary tests exceed their 5-second timeout under parallel host contention).
- 2026-09-13 Copilot: Reopened for breadcrumb optical alignment and transactional path-edit
  behavior requested after live review.
- 2026-09-13 Copilot: Shifted breadcrumb display and edit text up by 2px, made blur cancel the draft,
  and made Enter-only path navigation restore the prior loaded directory plus show the localized
  `pane.unableToOpenPath` toast on failure. Replaced the conflict modal's percentage max-height with
  fixed viewport gutters so its narrow layout no longer overflows by subpixels.
