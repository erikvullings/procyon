# 0311 Gallery lightbox

Status: open
Priority: low
Subsystem: frontend
Depends on: 0134

## Context
Thumbnails + grid (0134) and media streaming/preview (0087/0260) exist, but there's no
full-screen "photo-app" flip-through: open a selection of images/videos and page through them with
arrow keys, zoom, and minimal chrome. Great for curation and a big usability win for media libraries.

## Acceptance Criteria
- Enter a lightbox from a multi-selection or grid view: full-screen overlay, current item centred,
  left/right arrow (and J/K, swipe) to page, Esc to close returning focus/scroll to where you were.
- Supports images (zoom in/out + pan, fit/actual) and the existing video/audio players (0260/0262)
  inline; lazy-loads neighbours one step ahead.
- Thumbnail strip or count indicator ("3 / 12"); navigation respects the current sort/filter.
- Works in both hosts; keyboard-accessible (0066) with visible focus and aria roles.

## Implementation Notes
- Build on the grid's virtualized tile rendering (0134) and the F3 viewer (0087) for the actual
  decode/playback — the lightbox is a navigation shell over existing viewers, not a new renderer.
- Keep it a distinct route/tab state so it survives pane/tab memory (0270) and can open from either
  pane.

## Agent Notes
- (none yet)
