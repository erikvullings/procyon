# 0276 Show images on a map (folder or selection)

Status: open
Priority: medium
Subsystem: frontend
Depends on: 0273, 0275, 0134

## Context
With GPS data in images (0273) and a map component (0275), users can see where photos were taken: all geotagged images in the current folder, or just the selected one(s).

## Acceptance Criteria
- Action 'Show on map' for a single selected image, a multi-selection, or the whole folder (optionally recursive); geotagged images appear as thumbnail markers with clustering, images without GPS are counted and listed separately.
- Clicking a marker selects/reveals the entry in the pane; a hover or popup shows thumbnail, name, date and coordinates; selecting entries in the pane highlights markers.
- Dragging a marker (or placing an image without GPS by clicking the map) sets its coordinates via the metadata editor (0274), with confirmation.
- GPS extraction is a bounded, cancellable background job with progress, so large folders do not freeze the UI; results are cached by entry id and modified time.
- Works for local and remote entries; the thumbnail pipeline (0134) is reused.

## Implementation Notes
- Prefer reading only EXIF segments (a few KB per file) rather than whole images; run with limited concurrency, especially for remote providers.
- Consider a map tab/virtual view next to the grid view mode instead of only a dialog.

## Agent Notes
- Created 2026-10-18 from the image-metadata/map/GeoJSON discussion. No work started.
