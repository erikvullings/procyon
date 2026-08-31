# 0070 Favourites, bookmarks and recent locations

Status: done
Priority: low
Owner: unassigned
Agent: unassigned
Area: frontend
Depends on: 0069

## Context
`file-manager-coding-agent-spec.md` §16 milestone 3 and §37.

## Acceptance Criteria
- Users can bookmark the current location with a custom label; bookmarks persist through the
  settings service (0030) and are stored as `Location`s, not raw paths (§5.1).
- A favourites list is reachable from the toolbar, the command palette and a keyboard shortcut, and
  navigates the active pane on selection.
- Recent locations are tracked per workspace, deduplicated, bounded, and exclude locations the user
  has removed.
- Bookmarks and recents to locations that no longer exist are marked unavailable rather than
  silently failing on click.
- Reordering and deleting bookmarks is supported.
- Each bookmark also appears as an invokable action so it is palette- and shortcut-accessible (§18).
- Vitest tests: persistence round-trip, dedup/bounding of recents, unavailable-location handling.

## Implementation Notes
- Bookmarks must survive a settings schema migration (§26) — add a migration test.

## Agent Notes
- Added versioned settings persistence and migration coverage for named `Location` favourites and
  workspace-scoped recent locations.
- The tab-bar heart opens the favourites menu; double-clicking the breadcrumb edits and navigates
  pasted paths. Favourites can be added, removed, reordered, and opened alongside recent locations.
- Successful navigation records bounded, deduplicated recents. Failed location opens are visibly
  marked unavailable for the session. Favourites are also exposed as command-palette actions, with
  Ctrl/Cmd+Shift+H opening the palette.
- Verified with `cargo test -p fm-settings`, frontend type-checking, and focused Vitest suites.
  Repository-wide lint/test was started but could not complete while another Cargo process held the
  shared build lock.
- Final menu polish uses compact subdued text for selectable favourite and recent-location rows,
  retains brighter compact section headings, and keeps click targets and pointer cursors continuous.
- 2026-08-30: Fixed the menu at a responsive 18rem width so revealing favourite reorder/delete
  controls no longer changes the popup's size.
- 2026-08-30: Connection roots already listed under Cloud no longer offer a duplicate favourite.
  The Add favourite row now follows ordinary favourites, before Smart folders, and the menu can
  grow to 32rem before scrolling. macOS disk-image mounts are excluded from Volumes using
  `hdiutil` metadata, while normal removable drives remain available.
- 2026-08-30: Recent connection roots use their saved connection name instead of exposing the
  opaque connection UUID from the provider URI.
- `pnpm run lint` passes. A full frontend Vitest run is currently blocked by unrelated in-progress
  pane metadata integration: workspace/app-shell tests omit Pane's required `metadata` attribute;
  the mithril-inspector production-build test also fails.
