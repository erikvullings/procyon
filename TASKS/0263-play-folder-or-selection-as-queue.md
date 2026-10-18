# 0263 Play a folder or selection as a queue

Status: open
Priority: low
Subsystem: frontend
Depends on: 0262

## Context
Users should be able to play music without having a playlist file: select files or a folder and queue them.

## Acceptance Criteria
- Context-menu/command-palette actions: 'Play', 'Add to queue', 'Play next' for selected audio/video entries and for folders (non-recursive by default, recursive as option).
- Ordering follows the pane's current sort; non-media entries are ignored.
- Works from virtual folders (search results, basket, smart folders).

## Implementation Notes
- Reuse the action registry (0049) so the actions appear in menus, palette and keybindings.

## Agent Notes
- Created 2026-10-18 from the media/playlist feature discussion. No work started.
