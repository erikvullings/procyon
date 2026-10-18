# 0279 View GPX tracks, routes and waypoints on a map

Status: open
Priority: medium
Subsystem: frontend, backend
Depends on: 0275, 0088

## Context
GPX files (`.gpx`, GPS tracks from watches, phones and bike computers) open as XML text today. They should render on the map like GeoJSON (0277), with the activity data that makes GPX useful.

## Acceptance Criteria
- F3 viewer detects GPX and shows waypoints, routes and tracks (multiple segments) on the map, auto-fitted to bounds, with a toggle to the raw XML.
- Summary panel: distance, moving/elapsed time, elevation gain/loss, min/max elevation and speed when timestamps exist; an elevation (and optionally speed/heart-rate via the Garmin `gpxtpx` extension) profile chart whose hover position is shown on the map.
- Bounded and safe for big files: streaming or incremental parse, point-count limits with decimation for display (original data untouched), XXE-safe XML parsing, clear errors for invalid files.
- Time zones and missing timestamps are handled without wrong statistics (statistics are omitted rather than guessed).

## Implementation Notes
- Typed parser module shared with conversion (0281) — GPX 1.0 and 1.1; tests with real-world files from several devices.
- Reuse the map component (0275); keep the XML parser out of the UI thread (worker) for large files.

## Agent Notes
- Created 2026-10-18 from the image-metadata/map/GeoJSON discussion. No work started.
