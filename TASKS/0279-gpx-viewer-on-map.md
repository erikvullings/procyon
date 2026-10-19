# 0279 View GPX tracks, routes and waypoints on a map

Status: done
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
- Created 2026-10-18 from the image-metadata/map/GeoJSON discussion. 
- 2026-10-18 claude: fm-geo::parse_gpx (quick-xml event stream, namespace-prefix tolerant, GPX 1.0 and 1.1): waypoints -> Point, routes -> LineString, tracks -> LineString/MultiLineString (one feature per track, segments kept, 3D only when every point has elevation); properties name/desc/cmt/type/sym/src/link/ele/time; Garmin gpxtpx hr read for the profile. XML safety: entities never expanded, DOCTYPE with ENTITY refused, syntax errors carry line/column. Bounds: 64 MiB file cap (parsed on the backend, not the UI thread), lines thinned to 50k points per track for DRAWING only (warning shown) while statistics use every point. Statistics (track_stats.rs): distance (haversine, segments not joined), elapsed, moving time (>=0.5 m/s), average moving speed, top speed over >=10 s windows, ascent/descent with a 3 m noise floor, min/max elevation, start/end UTC; anything unsupported is omitted rather than guessed (no elevation -> no ascent/profile; missing, zone-less or backwards timestamps -> no durations/speeds, plus a warning). Timestamps with offsets are normalised to UTC (own ISO-8601 parser, tested). API: GeoDocumentDto.tracks (+ <=500-sample profile with position, elevation, speed, hr). Frontend: .gpx opens in the 0277 GeoView (map auto-fitted, raw XML toggle via the shared header button); per-track summary panel and an SVG profile chart (elevation/speed/heart-rate tabs) whose pointer or Left/Right-key position drives a cursor marker on the map (new MapView cursor attr). Not done: parse is event-streamed but the file is read fully into memory first (cap 64 MiB); only hr is read from extensions (no cadence/temperature); fixtures are hand-written to mimic Garmin and GPX 1.0 files, not real device exports; Edit button not offered for GPX (conversion arrives with 0281).
