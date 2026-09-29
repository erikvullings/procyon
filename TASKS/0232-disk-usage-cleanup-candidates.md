# 0232 Disk usage: clean-up candidates

Status: done
Priority: medium
Subsystem: backend
Depends on: 0118, 0231

## Context
BlitzTree's `cleanup.rs` recognises regenerable folders from scan structure alone
(`node_modules`, `.venv`/`venv` with `pyvenv.cfg`, Rust `target` next to `Cargo.toml`, `.next` next
to `package.json`, Xcode `DerivedData`, `*DeviceSupport`, `Library/Caches`, tool caches) above a
size threshold. Showing these in our disk-usage view gives users quick wins.

## Acceptance Criteria
- The scan response (and final progress event) carries a bounded, size-sorted list of clean-up
  candidates with location, kind and sizes, computed from the full scanned tree (not the
  depth-capped response). Nested candidates are not double-reported.
- The disk-usage view shows a "Clean up" panel listing candidates with a description per kind and
  actions to open the folder in the opposite pane and to move it to Trash through the existing
  delete operation (with its confirmation).
- Rules are heuristics only and never delete anything by themselves.

## Implementation Notes
- Backend rules live in a dedicated module under `crates/fm-application/src/`.

## Agent Notes
- Rules live in `crates/fm-application/src/disk_usage_cleanup.rs`, which works over a small
  `CleanupTreeNode` trait so it's unit-testable without a filesystem. `ScanTree` implements it, and
  candidates are computed from the full scanned tree at finalisation. Matched folders are not
  descended into, the scan root itself is never a candidate, and the list is sorted by physical
  size and capped at 100. The threshold is 50 MB of physical size.
- Rules: `node_modules` and `.venv` by name; `venv`/`env` only with a `pyvenv.cfg` child; `target`
  only beside `Cargo.toml`; `.next` only beside `package.json`; `Xcode/DerivedData` and
  `Xcode/*DeviceSupport`; `Caches` under `Library` or `CoreSimulator`; `~/.cache` (its grandparent
  is `home` or `Users`); and `.npm/_cacache`, `.gradle/caches` and `.pnpm-store` as tool caches.
- Transport: `ScanDiskUsageResponseDto.cleanupCandidates` and `DiskUsageProgress.cleanupCandidates`
  both use `serde(default)`. Intermediate snapshots send an empty list; only the final result
  carries candidates. When a collapsed folder is expanded, `mergeCleanupCandidates`
  (`features/disk-usage/cleanup-candidates.ts`) replaces the candidates under that folder with the
  rescan's.
- UI: the toolbar shows a "Clean up (count · size)" toggle that opens a panel. Each row has "Show"
  (opens the folder in the other pane) and "Move to Trash", which calls
  `OpsController.trash([location])` and so goes through the normal Trash confirmation. The button
  is shown only for `local`/`file` locations. A trashed row is hidden locally once the operation
  starts. Treemap sizes stay stale until the next scan; this is deliberate, so the tree never shows
  sizes the backend hasn't measured.
- The mock applies only the `node_modules` rule and ignores the threshold, because fixture sizes
  are tiny.
- Verified in the mock dev server by temporarily marking fixture folders as candidates: the toggle,
  the panel, the Trash confirmation, and the row being hidden after confirming.
