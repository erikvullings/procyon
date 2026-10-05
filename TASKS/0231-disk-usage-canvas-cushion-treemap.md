# 0231 Disk usage: canvas cushion treemap with zoom and richer colours

Status: done
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
  in the opposite pane remains available (the item list and the toolbar).
- A keyboard-accessible list of the zoomed folder's largest items stays in sync with the map.
- Files are coloured by extended type groups (video, images, audio, archives, binaries, code,
  documents, databases, system files, other) via theme CSS variables.
- Layout, shading, hit-testing and colour classification are unit-tested; the view works in jsdom
  where `<canvas>` has no 2D context.

## Implementation Notes
- `frontend/src/features/disk-usage/` (layout, renderer, view, CSS) and `themes/theme.css`.

## Agent Notes
- 2026-09-29 copilot: `cushion-treemap.ts` builds a `TreemapScene` (squarified layout in device
  pixels, cushion surfaces, per-tile trails, title strips) and paints it into an RGBA buffer;
  `hitTestTreemap` returns the deepest tile under a CSS-pixel point. Headed directories need
  depth ≥ 1 and room for a title strip; chains where one child holds ≥ 99% collapse into
  "A ▸ B"; unheaded directories get a darkened frame instead. The visible-children cutoff is
  0.05% on canvas (was 0.5% for SVG) so depth is bounded only by pixels. Attribution comment to
  BlitzTree (MIT) is in the renderer.
- `file-type-colours.ts` classifies names into ten groups, reads `--fm-disk-usage-<role>` from the
  theme (light, dark and prefers-dark blocks in `theme.css`), and gives unknown extensions a stable
  hashed hue so different unknown types stay distinguishable.
- The view caches the scene by (node, size, device scale ≤ 2, palette) and repaints only when it
  changes. Clicking the map zooms into the top-level folder under the pointer (collapsed folders
  are rescanned with `onExpandFolder`); breadcrumbs, the ↑ button and Backspace/Escape zoom out.
  Double-click was dropped because single-click already zooms; opening a folder in the other pane
  uses each row's ↗ button or the toolbar's "Open in other pane" for the zoomed folder.
- The workspace pane's click handler focuses its section after the canvas click, so the canvas host
  reclaims focus in a `setTimeout` (a microtask runs too early, between listeners) so Backspace
  and Escape reach the view.
- Visually checked in `pnpm dev:mock` (light and dark). jsdom has no 2D context: the view guards a
  null context, and the tests stub `getContext`/`ImageData`.
- 2026-10-05: Folder selection now both zooms the treemap and opens the folder in the
  originating pane; file selection opens its parent there with the cursor on the file.
  The treemap remains in the opposite pane during both actions.
