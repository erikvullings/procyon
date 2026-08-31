# 0169 Saved advanced filter presets

Status: open
Priority: low
Subsystem: frontend, settings
Depends on: 0030, 0067

## Context

Quick Filter provides an ad hoc text match, while 0129 tracks Total Commander's saved-filter feature
only as an untriaged candidate. Users should be able to save reusable pane filters without launching
a recursive search or smart folder.

## Acceptance Criteria

- Quick Filter supports a structured advanced mode for name/glob, type, size, modified age, hidden
  state, and platform-aware executable status where available.
- Users can save the current filter under a unique name, load it in either pane, rename it, and delete
  it.
- Applying a preset filters the current directory's loaded/paged results without recursively walking
  subdirectories.
- Active filter criteria remain visible and can be cleared in one action; unsupported criteria are
  explained rather than ignored.
- Presets persist through the existing versioned settings model and older settings migrate to an
  empty preset list.
- Tests cover predicate combinations, preset persistence, overwrite confirmation, provider metadata
  gaps, paging interaction, and keyboard accessibility.

## Implementation Notes

- Split from the Ctrl+F12 candidate in 0129; update that parent task when this feature is completed.
- Keep this distinct from 0162: filters constrain the current directory view, while smart folders run
  reusable searches across a scope.
- Reuse the saved-preset UX and migration patterns established by 0149.

## Agent Notes

- 2026-08-28: Promoted from 0129 into a standalone task during the product feature review.
