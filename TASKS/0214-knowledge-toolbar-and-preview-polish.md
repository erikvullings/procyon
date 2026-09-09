# 0214 Knowledge toolbar and preview polish

Status: done
Priority: high
Subsystem: frontend, semantic search, preview
Depends on: 0212

## Context

The compact Search Knowledge pane introduced in task 0212 now fits the pane, but several controls
still differ from the surrounding pane chrome. The subject field retains a focus ring the user does
not want in this toolbar, the search action occupies textual space, and source positions can be
missing for production provenance wrappers. Viewer tabs must also retain the normal new-tab and
favourites affordances.

The file preview's initial loading state currently renders unpadded text at the top-left. It should
use the application's existing progress component in the centre of the pane. The custom
heart-plus glyph also diverges from the current official Tabler outline and looks deformed.

## Acceptance Criteria

- Remove the visible blue focus treatment from the compact Search Knowledge subject field.
- Replace the textual Search action with an accessible Enter-style `IconButton`.
- Align the Search and settings icon buttons to the same row-height geometry and spacing as the
  pane's new-tab and favourites buttons.
- Render clickable page/chapter/section positions for both direct and production wrapped evidence
  provenance, preserving exact source navigation.
- Keep the pane-level new-tab and favourites controls visible while Search Knowledge or another
  viewer tab is active.
- Replace the file preview's unpadded loading text with a horizontally and vertically centred,
  accessible indeterminate spinner.
- Replace the custom heart-plus path with the current official Tabler outline glyph.

## Agent Notes

- 2026-09-09 Copilot: Interpreted the request's contradictory “should not be added” wording from
  its stated keyboard-only problem: the pane-level new-tab and favourites controls must remain
  available for Search Knowledge, rather than being inserted into the search form itself.
- 2026-09-09 Copilot: Replaced the textual action with an accessible Tabler Enter icon, removed
  the subject field's focus ring, and matched both search controls to the pane's row-height action
  geometry. Updated heart-plus to the current official Tabler path.
- 2026-09-09 Copilot: Replaced preview loading text with a centred
  `mithril-materialized` indeterminate spinner. Added regressions for the loader, viewer-tab
  actions, production exact/span provenance labels, toolbar semantics, geometry, and focus styling.
- 2026-09-09 Copilot: Verified the live Tauri app against the retained semantic index: Search,
  settings, new-tab, favourites, and `Open Page 176 in source` were exposed; activating the
  evidence link opened the existing PDF at page 176 of 199. The 281 focused frontend tests,
  frontend typecheck, repository Rust/Biome lint, and patch checks pass. The Impeccable detector
  reported only two unrelated pre-existing theme warnings.
