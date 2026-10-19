# 0276 Show images on a map (folder or selection)

Status: done
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
- Created 2026-10-18 from the image-metadata/map/GeoJSON discussion. 
- 2026-10-18 claude: Backend: POST /api/v1/files/image-gps (+Tauri read_image_gps, client.readImageGps on all adapters) reads GPS + capture time of up to 100 images per request from only the first 512 KiB through the VFS (local/archive/remote alike); unreadable files are flagged, not fatal. Frontend: 'Show on map' action (context menu + palette; selection, or whole active folder when nothing selected; 'Include subfolders' re-scan, 5000-image cap with notice) opens ImageMapPanel above the function-key bar: scan is a bounded (25/batch, 2 in flight), cancellable job with progress, results cached by entry id + mtime + size (GpsCache); located images are thumbnail markers (0134 pipeline, bounded concurrency) with clustering via the 0275 MapView; popup shows thumbnail, name, date taken, coordinates; marker click selects the entry in the pane (message if the entry is not in the listed folder); pane selection highlights markers; images without GPS are listed beside the map and counted; unreadable ones counted. Dragging a marker (JPEG/TIFF/WebP) or choosing 'Place on map' + clicking the map asks for confirmation, then runs editImageMetadata (0274) with a .orig backup and rescans; PNG/HEIC are not draggable (toast explains). The F3 viewer's 'Open on map' (0273) now opens this panel for the viewed image. Limits: no dedicated map tab/virtual view (inline panel, like duplicates/snapshots); recursive scan lists folders through the pane-list API; real map rendering/drag untested outside mocked MapLibre.
