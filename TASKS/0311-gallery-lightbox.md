# 0311 Gallery lightbox

Status: in_progress
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
- 2026-11-08 claude: Built the navigation shell inside the existing F3 viewer (no new renderer):
  `features/preview/gallery.ts` (media filter + neighbours in pane order), `previewWiring.galleryOf/
  stepGallery` (listing = the sibling tab the viewer tab borrowed its folder from, sorted + quick-
  filtered like the pane via `visibleEntries`), key route `viewer-gallery` (J/K for every media kind;
  ArrowLeft/Right only for a fitted image, zoomed images keep panning), header "‹ 3 of 12 ›" with
  disabled ends (i18n x8). Tests: gallery.test.ts (3), keydown routes (3), viewer header (2).
  **Not done (criteria still open):** true full-screen overlay/minimal chrome, swipe, lazy preloading
  of the neighbour, thumbnail strip, entering from a multi-selection (only flips over the whole
  folder's media), focus/scroll return on Esc beyond the existing viewer close, survival in tab
  memory (0270). Not verified visually or in a real host.
