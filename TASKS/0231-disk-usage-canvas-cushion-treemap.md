# 0231 Disk usage: canvas cushion treemap with zoom and richer colours

Status: open
Priority: medium
Subsystem: frontend
Depends on: 0118

## Context
Our treemap renders one SVG `<rect>` per tile and caps render depth at 3. BlitzTree paints a
WinDirStat-style cushion treemap into a bitmap once (every tile adds a parabolic ridge to a
quadratic surface; pixels are shaded from the surface normal), hit-tests against the laid-out
rectangles, zooms into folders, and colours files by a richer set of type groups.

## Acceptance Criteria
- Treemap is painted to a `<canvas>` at device-pixel resolution with cushion shading, framed
  directory title strips, and "A ▸ B" collapsing of single-child directory chains.
- Rendering depth is limited only by the data and a sub-pixel cutoff, not a fixed depth of 3.
- Clicking a directory zooms into it; a breadcrumb and Backspace/Escape zoom out. Opening a folder
  in the opposite pane remains available (double-click and the item list).
- A keyboard-accessible list of the zoomed folder's largest items stays in sync with the map.
- Files are coloured by extended type groups (video, images, audio, archives, binaries, code,
  documents, databases, system files, other) via theme CSS variables.
- Layout, shading, hit-testing and colour classification are unit-tested; the view works in jsdom
  where `<canvas>` has no 2D context.

## Implementation Notes
- `frontend/src/features/disk-usage/` (layout, renderer, view, CSS) and `themes/theme.css`.

## Agent Notes
