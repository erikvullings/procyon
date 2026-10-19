# 0275 Map component with a replaceable tile source

Status: done
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
- Created 2026-10-18 from the image-metadata/map/GeoJSON discussion. 
- 2026-10-18 claude: Shared map: features/map/map-view.ts (Mithril, MapLibre GL JS 6 loaded via dynamic import - separate 1 MB lazy chunk, nothing requested until a map mounts; DOM markers with supercluster clustering, popups, drag, selection highlight, fit-to-bounds, light/dark via CSS filter on imagery, attribution always visible non-compact). Basemap settings: fm-settings MapSettings (online tiles switch; raster URL template + attribution + max zoom | MapLibre style URL | local raster PMTiles via VFS media streaming) in SettingsDto.map, validated in Rust (https or localhost only, no credentials, placeholders, attribution required) and mirrored in the settings editor 'Map' section with reset-to-OSM default, network notice naming the host, preview map button. Default OSM raster tiles are fetched by a BACKEND proxy (fm-application map_tiles: user agent, max 2 concurrent, 1 MiB cap, image/* only, 512-tile memory cache, no prefetch) exposed as GET /api/v1/map/tiles/{z}/{x}/{y} and Tauri procyon-tiles scheme, so the web view never contacts the provider (no CSP exceptions needed; the app has no CSP today). Offline switch -> only PMTiles or a blank graticule grid, no URL in the style (tested). Style-URL kind is fetched directly by the web view (UI says so). Not done / honest limits: MBTiles unsupported (PMTiles only, raster only; vector PMTiles needs a style); real WebGL rendering, tile loading and Tauri scheme not verifiable in jsdom/CI (MapLibre mocked); no explicit CSP added; no consumer yet (0276+ use it).
