# 0258 Directory refresh amplification (large dirs, post-operation)

Status: open
Priority: high
Subsystem: backend, frontend
Depends on: 0156

## Context
Navigation/refresh feels slow in the Tauri app. Profiling by code reading plus `fm-vfs-local` bench (2026-10-18): raw listing is cheap (10k entries stat+list ≈ 15 ms, 100k read_dir ≈ 52 ms), so the cost is repeated work and IPC round trips, not the filesystem or webview.

Findings (file refs):
1. `LIST_PAGE_SIZE = 256` (`crates/fm-application/src/directory.rs:55`) is also the delta threshold in `deltas_for_change`: any directory with >256 entries never gets incremental deltas, only `Reset` with page one.
2. Frontend `applyDelta` (`frontend/src/app/app-shell.ts` ~2090) answers a Reset with `hasMore` (or any delta while `current.hasMore`) by `refetchAffectedPanes` -> `navigation.load` -> backend `list()` with no continuation token = a second full relist + sort, then `hydrateBackgroundSnapshot` fetches remaining pages serially (`navigation.ts` ~453). Result per fs event in a big folder: watcher relist + frontend relist + N/256 sequential IPC calls.
3. Operation completion triggers three refreshes per affected pane: backend `refresh_affected` (ResetRequired relist), frontend foreground `refetchAffectedPanes` (`panesNeedRefresh` in `backend-event-handler.ts`), and the FS watcher's own relist.
4. `list()` cost per continuation page: `provider.inspect` writability stat and `aggregate_totals` over the full list on every page (O(n^2/256)); `publish_changes` clones the page/snapshot several times and re-sorts the full list each event.
5. `hydrateBackgroundSnapshot` re-spreads `mergedEntries` per page (O(n^2/256)); `applyDelta` rebuilds a Map of all entries and appends added entries unsorted (verify whether the table re-sorts client-side).
6. Watch debounce (75 ms, `fm-vfs-local/src/lib.rs`) is leading-edge only; bursts still relist repeatedly.

## Acceptance Criteria
- Measure first: add `performance.mark`/Rust `tracing` spans for navigate -> first paint and fs-event -> repaint; record baseline for 300, 10k, 100k-entry dirs in this file.
- Large directories use incremental deltas (diff against full cached list) or a single bounded Reset; no frontend relist on a normal delta.
- Frontend never triggers a full backend relist in response to a backend-originated delta; remaining pages are served from the cached `full_entries`.
- One refresh per affected pane per operation (dedupe backend reset, watcher event, and frontend refetch).
- Continuation pages skip the writability stat and totals recompute (cache in `PaneRequest`).
- Trailing-edge debounce/coalescing of watch bursts; before/after numbers recorded; `pnpm test` and `pnpm lint` green.

## Implementation Notes
- Keep logic in `DirectoryService`; keep Tauri/Axum handlers thin. Preserve browser/Tauri parity.
- Consider binary/compact transport for pages if IPC serialization shows up in the traces.

## Agent Notes
- Created 2026-10-18 from profiling review; no code changed yet.
