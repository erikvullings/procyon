# 0263 Play a folder or selection as a queue

Status: done
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
- Created 2026-10-18 from the media/playlist feature discussion. 
- 2026-10-18 claude: Frontend-only client actions (features/player/player-actions.ts): client.play / client.playNext / client.addToQueue (file context menu, palette, bindable; no default chords) plus palette-only client.playRecursive / client.addToQueueRecursive. features/player/play-selection.ts collects audio+video files from the pane's selection (or cursor) in displayed order and lists folders with the tab's sort/hidden/foldersFirst via throwaway-pane listDirectory (paged, depth 8, 5000-track cap, symlinks never followed, dedupe); basket/search/virtual panes work because their entries carry real locations. Availability: needs a media file or folder. Wired through generic invokeClientAction in the shell. Notes: spec said action registry; chosen the existing client.* mechanism (no Rust registry change, no parity gap since both hosts share the frontend). Video files queue as audio-only (the mini-player is an <audio> element). Not exercised in a live UI.
