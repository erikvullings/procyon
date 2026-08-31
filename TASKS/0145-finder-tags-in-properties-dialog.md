# 0145 Surface Finder tags/Spotlight comment editing in the Properties dialog

Status: open
Priority: low
Owner: unassigned
Agent: unassigned
Area: frontend
Depends on: 0136, 0140

## Context

Split out of [0136](0136-extended-attributes-and-finder-tags.md). 0136 shipped its own two minimal
standalone dialogs (`frontend/src/features/entry-metadata/finder-tags-dialog.ts` and
`spotlight-comment-dialog.ts`, reachable via the selection context menu's "Edit Tags…"/"Edit
Comment…" entries and the command palette) because [0140](0140-properties-dialog.md)'s Properties
dialog didn't exist yet when 0136 started — it landed on `main` mid-task. 0140's own Implementation
Notes explicitly anticipated this split: "Check whether 0136 ... wants to surface its data through
this same dialog rather than a separate surface — likely yes, but not a hard dependency."

This task folds the two standalone dialogs' functionality into `PropertiesDialog`
(`frontend/src/features/properties/properties-dialog.ts`), which today only *displays* metadata
(general/permissions/archive sections, all read-only, populated from a single `getEntryMetadata`
call) — it has no precedent yet for an editable field or a section that issues its own separate
network calls.

## Acceptance Criteria
- `PropertiesDialog`, for a single-entry selection only (not the multi-selection aggregate view),
  gains a "Tags" section and a "Comment" section, each visible only when the corresponding
  capability (`runtimeCapabilities().finderTags` / `.extendedAttributes`) is true — matching how
  every other capability-gated affordance in this app behaves, never present-but-broken.
- Tags section: fetches via `client.getFinderTags(entry.location.uri)` when the dialog opens for
  that entry (mirror the existing `requestedEntryId`-keyed fetch-on-open pattern already used for
  `getEntryMetadata`, including abort-on-close/entry-change). Shows the current tags (reuse the
  chip-list rendering from `finder-tags-dialog.ts`'s `FinderTagsDialog` rather than re-deriving it)
  with an inline add/remove/color-picker editor - either embedded directly in the properties body,
  or an "Edit Tags…" button that still opens the existing `FinderTagsDialog` as a nested dialog.
  Either is acceptable; prefer whichever produces less duplicated markup/state.
- Comment section: same fetch-on-open pattern via `client.getSpotlightComment(...)`, an editable
  textarea, and a Save action that calls `client.setSpotlightComment(...)` and updates the visible
  value on success.
- After a successful tag edit, call the existing `FinderTagsLoader.setCached(uri, tags)` (already
  exposed on the loader for exactly this purpose - see its doc comment) so the directory table's
  tag-dot badge updates immediately without waiting for a refetch; `PropertiesDialog` needs a way to
  reach the active pane's `FinderTagsLoader` instance, which it does not currently receive as an
  attr.
- Decide whether the standalone `core.editFinderTags`/`core.editSpotlightComment` context-menu
  actions and their dialogs should be removed once this lands (redundant with Alt+Enter → Properties
  → edit), or kept as a faster one-click path — check with the user/task reviewer before deleting a
  working, tested surface; this task can ship additively (Properties dialog gains the capability)
  without removing the existing shortcut either way.
- Tests: dialog rendering for a single entry with tags/a comment present, capability-gated hiding
  when `finderTags`/`extendedAttributes` are false, and the edit round-trip (mock client) updating
  both the dialog's own display and (if kept) the loader cache.

## Implementation Notes
- `PropertiesDialogAttrs` currently takes `client: PropertiesMetadataClient` (a narrow
  `getEntryMetadata`-only slice of `FileManagerClient`, for testability) — widen that interface (or
  add a second narrow slice type) to include `getFinderTags`/`setFinderTags`/
  `getSpotlightComment`/`setSpotlightComment`, following the same "narrowest interface the
  component actually needs" convention already established there.
- `frontend/src/features/dialogs/app-dialogs.ts` already renders both `PropertiesDialog` and the
  two standalone dialogs side by side (search for `FinderTagsDialog`/`SpotlightCommentDialog`); this
  is where `PropertiesDialog`'s attrs get assembled, so it's the place to thread through a
  `FinderTagsLoader` reference if the cache-update acceptance criterion above needs it.
- `frontend/src/features/directory-table/finder-tag-colors.ts` (color→CSS-custom-property mapping)
  and the chip-rendering markup in `finder-tags-dialog.ts` should be reused as-is, not
  re-implemented — extract a small shared render helper if embedding inline rather than nesting the
  existing dialog.

## Agent Notes
- (none yet)
