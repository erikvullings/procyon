# 0118 Integrate parallel-disk-usage with WinDirStat Treemap View

Status: done
Priority: high
Subsystem: backend
Depends on: none

## Context

The application needs a disk space analysis feature that calculates filesystem node sizes efficiently and visualizes them in a WinDirStat-style treemap view (cushion treemap / visual block layout of relative file sizes). We will leverage `parallel-disk-usage` as the core engine for high-performance multi-threaded traversal, hardlink handling, and size aggregation, and stream or pass the resulting hierarchical node graph into a UI component capable of rendering a interactive visual treemap.

## Acceptance Criteria

- Integrate `parallel-disk-usage` (or execute as subprocess/library binding) to perform fast multi-threaded disk usage scans over a designated root path.
- Construct a hierarchical JSON/struct tree mapping directories and files to their physical/logical disk usage.
- Map the hierarchical disk structure into a WinDirStat-like treemap UI component (using Squarified Treemap layout algorithm).
- Implement interactive elements: hovering shows file details/size, clicking selects/navigates into the subtree, and color-coding groups files by type/extension.
- Support non-blocking asynchronous scanning so the file manager UI remains responsive during disk traversal.

## Implementation Notes

- **Backend Traversal Engine:** Use `parallel-disk-usage` crate dependency in Rust or parse its `--json-output` stream into internal tree data structures.
- **Treemap Layout:** Implement or integrate a Squarified Treemap layout algorithm to dynamically calculate `(x, y, width, height)` bounding boxes for each file/folder relative to container bounds.
- **UI & Visualization:** Render tree rectangles using canvas/SVG/native drawing commands with color schemes derived from file extensions (e.g., media files, code, executables, archives).
- **Performance Considerations:** Cap maximum display depth or aggregate micro-files (smaller than 0.5% screen area) into a "small files" bucket to prevent rendering bottlenecks.

## Agent Notes

- Initial task setup based on feature request for `parallel-disk-usage` + WinDirStat visual treemap integration.
- 2026-08-27 copilot: Integrated `parallel-disk-usage` 0.24 as a library with its CLI features
  disabled. `fm-application` runs separate apparent-size and allocated-size traversals inside
  `spawn_blocking`, deduplicates Unix hardlinks, reports unreadable entries, and maps the result to
  recursive transport DTOs. The retained hierarchy is capped at depth 12 while directory totals
  continue to include descendants below that display cap; hardlink corrections are reconciled
  through capped branches. Wide directories retain at most 2,048 children, aggregating overflow
  into one block before transport. The endpoint is available through Axum, Tauri, and every
  `FileManagerClient` adapter; OpenAPI and Orval output were regenerated.

  The frontend implements a squarified SVG treemap with extension-group colours, logical/physical
  hover details, keyboard-accessible blocks, a 0.5% small-files bucket, loading/retry/error states,
  and a second depth guard. Ctrl/Cmd+Shift+L (also available in the command palette) creates a
  separate transient tab in the active pane. Selecting a directory block opens that directory in
  the opposite pane; selecting a file or aggregated block opens its containing directory there.
  Closing the tab aborts its HTTP request and stale responses are ignored. The scanner itself is a
  synchronous Rayon traversal, so work already running inside `spawn_blocking` cannot be forcibly
  interrupted; closing a tab prevents its result from being applied but may not immediately stop
  filesystem work.

  Verified with backend hierarchy, depth-cap, hardlink, DTO, Axum, and Tauri tests; generated API
  freshness; frontend layout/view, keybinding, command-controller, client-adapter, and AppShell
  integration tests; frontend type checking; and the repository-wide lint command. The full Rust
  suite and all 1,453 frontend tests pass. The script suite retains two unrelated baseline failures:
  its CI test still expects `cargo test --workspace` although CI uses nextest, and its packaging test
  expects the former `dev.fm.desktop` identifier instead of the configured
  `nl.erikvullings.procyon`.
- 2026-08-27 copilot: Follow-up fixed both stale script assertions. The CI contract now checks
  `cargo nextest run --workspace` plus the separate doctest command, and the desktop packaging
  contract verifies that Cargo metadata, the Tauri bootstrap config, and the generated build
  overlay agree on the current product identity, workspace version, and icon set.
- 2026-08-28 copilot: Follow-up removed the duplicate local filesystem traversal. One
  `parallel-disk-usage` pass now captures logical and allocated bytes from each metadata result,
  preserving hardlink handling and hierarchy totals. While the one-shot scan is running, the
  frontend shows an animated activity indicator and updates elapsed time every second instead of
  presenting a static loading message.
- 2026-08-28 copilot: Follow-up scans immediate root entries concurrently and streams
  workspace-scoped partial trees after the first completed entry, then on a progressively slower
  250 ms, 500 ms, 1 s, 2 s, and 4 s cadence. Stable recursive name ordering and frontend subtree
  merging reduce layout movement while the treemap updates. `.git`, `.hg`, `.svn`, and
  `node_modules` retain their exact aggregate size but hide descendants until the user selects the
  dedicated Expand action. HTTP, Tauri, and mock transports share scan correlation and reject stale
  or late events; terminal results globally reconcile hardlinks before completing the scan.
- 2026 copilot (backend regression fix): A real home-directory scan reached 1328% CPU because
  each top-level subtree's worker independently called `parallel_disk_usage::FsTreeBuilder`, which
  itself forks into Rayon's *global* pool at every directory level — the old
  `available_parallelism()`-many std-thread fan-out multiplied that into genuinely unbounded nested
  parallelism. Replaced `FsTreeBuilder` with a hand-written sequential recursive
  `build_tree_sequential` (still producing the same `parallel-disk-usage` `DataTree`/hardlink
  shapes) run by a hard cap of `DISK_USAGE_WORKER_COUNT = 2` std-thread workers total, regardless of
  how many top-level subtrees exist. The traversal checks a `tokio_util::sync::CancellationToken`
  at every entry/directory it visits and returns `ApplicationError::OperationCancelled` promptly.
  Cancellation is now real: a new `DiskUsageCoordinator` (owned by `FileManagerService`, not static
  state) registers `scan_id -> CancellationToken`, rejects a `scan_id` that is already running, and
  exposes `cancel_disk_usage(scan_id)`; a `ScanGuard` RAII type cancels and unregisters the token on
  `Drop`, so dropping/aborting the async scan future (an aborted Tauri command, a disconnected Axum
  request) also stops the blocking traversal instead of letting it run to completion unobserved.
  Added `DELETE /api/v1/directories/disk-usage/{scanId}` (204/404) and a `cancel_disk_usage` Tauri
  command, registered in both `invoke_handler!` lists. `ScanDiskUsageResponseDto` and the
  `DiskUsageProgress` event gained `scanned_entries: u64`, incremented per traversed entry so
  repeated progress snapshots visibly advance even while no top-level subtree has completed yet
  (fixed `coordinate_progress`'s cadence gate, which previously required a *new* completed subtree
  before it would ever re-arm its timeout tick). Unreadable reporting is no longer count-only: a new
  `DiskUsageUnreadableEntryDto { location, reason: permissionDenied|disappeared|ioError }` is
  recorded for metadata/`read_dir` failures (sanitized from raw `io::ErrorKind`, no OS error
  strings), capped at 500 details, stable-sorted by location, and included in both the final
  response and progress events; `unreadable_entries` count is retained unchanged for backward
  compatibility. This also relaxes the old behavior where any io error kind other than
  `NotFound`/`PermissionDenied` aborted the entire subtree with `ApplicationError::Internal` — all
  io error kinds are now recorded and traversal continues. Added regression tests: worker-count
  bound (`effective_worker_count`), cancellation interrupting a 20k-entry deterministic fixture (at
  both the `build_tree_sequential` seam and the full `scan_local_tree`/HTTP-route seam), a dedicated
  drop-cancels-work test (`tokio::spawn` + `JoinHandle::abort`, since an `async fn`'s body does not
  run until first polled so a bare `drop` of the unpolled future would prove nothing), unreadable
  detail mapping/capping, and `scanned_entries` advancing across `snapshot_response` calls with no
  new completed subtree. All existing disk-usage tests (backend and host) pass unmodified. This
  round only touched Rust backend/DTO/host files and tests — `frontend/**` and the generated
  OpenAPI/Orval output were intentionally left untouched per this session's scope and are now stale
  relative to the new `DELETE` route and DTO fields; a follow-up frontend session must run
  `pnpm api:export`/`pnpm api:generate` and wire up the new cancel affordance and unreadable-details
  UI before this reaches parity end-to-end.
