# 0275 Map component with a replaceable tile source

Status: open
Priority: medium
Subsystem: frontend, backend
Depends on: 0030

## Context
Several features need a map (images on a map, GeoJSON viewing and editing). Build one shared map component on MapLibre GL JS (project-preferred stack) with a free default basemap that users can replace, and nothing hardcoded to one provider.

## Acceptance Criteria
- Shared `map-view` component (Mithril, typed, no shared state held locally) wrapping MapLibre GL JS with markers, clustering, popups, fit-to-bounds and light/dark theming.
- Settings expose the basemap: a style URL or raster tile URL template (with attribution text and max zoom), with a free default (e.g. OpenStreetMap-based raster tiles or an open vector style); also supports a local PMTiles/MBTiles file for fully offline use. Validation and a 'reset to default' action.
- Attribution is always displayed as the provider requires; the default respects the provider's tile usage policy (user-agent/referrer, no bulk prefetch) and the UI says which host receives requests.
- No network request is made until a map is actually opened; a setting can disable online tiles entirely (then only offline sources or a blank grid).
- Works in browser and Tauri hosts; the CSP / Tauri capabilities allow only the configured tile host(s).

## Implementation Notes
- Add `maplibre-gl` as a dependency (check bundle size; load lazily via dynamic import so startup is unaffected — see the startup profiling in 0259).
- Settings go through the settings migration machinery (0030) and i18n (0098); keep tile URLs out of logs.

## Agent Notes
- Created 2026-10-18 from the image-metadata/map/GeoJSON discussion. No work started.
