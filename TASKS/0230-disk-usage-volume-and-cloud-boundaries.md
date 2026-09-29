# 0230 Disk usage: stop at volume and cloud-only boundaries

Status: done
Priority: high
Subsystem: backend
Depends on: 0118

## Context
Review of [BlitzTree](https://github.com/ahmedkhaleel2004/blitztree) (MIT) showed two correctness
gaps in our local disk-usage scanner (`crates/fm-application/src/disk_usage.rs`):

- It descends into directories whose contents are cloud-only (macOS `SF_DATALESS`, iCloud Drive /
  File Provider). Listing such a directory asks the provider to materialise it, so scanning `~`
  can trigger downloads.
- It crosses mount points (different `st_dev`), so scanning `/` wanders into disk images, network
  shares and `/Volumes`. A scan should measure one volume.

## Acceptance Criteria
- Directories on a different device than the scan root are not descended; their own entry size is
  still counted and they are reported as skipped with reason `otherVolume`.
- On macOS, directories flagged `SF_DATALESS` (and a cloud-only scan root) are not listed and are
  reported with reason `cloudOnly`.
- New reasons flow through DTO, event payload, OpenAPI/Orval output and all eight locales.
- Pure boundary decision is unit-tested; existing disk-usage tests keep passing.

## Implementation Notes
- Windows junctions/symlinks are already reported by `symlink_metadata` as symlinks and not
  followed, so the device check is `cfg(unix)` only.

## Agent Notes
- 2026-09-29 copilot: `crates/fm-application/src/disk_usage.rs` gained `ScanBoundary` (root
  device fixed from the scan root) and the pure `boundary_skip_reason`. `build_tree_parallel`
  checks it after reading a directory's own metadata, so the directory's entry size still counts
  but it is never listed; a cloud-only scan root yields an empty tree plus one `cloudOnly` entry.
  Skips reuse the unreadable registry with new reasons `otherVolume`/`cloudOnly` (DTO, event
  payload, all locales; the toolbar label now says "skipped"). Verified manually on macOS that
  scanning `/Volumes` reports the external `My Passport` mount as `otherVolume`.
- The disk-usage response DTOs were never in the OpenAPI document (results only arrive through
  events), so `frontend/src/api/generated/models/diskUsage*.ts` were orphaned files that
  `api:generate` no longer refreshed. `apps/fm-server/src/routes/mod.rs` now registers
  `ScanDiskUsageResponseDto` as a component schema; the models regenerate from the backend again.
- No automated test can create an `SF_DATALESS` directory or a second device in a temp root, so
  those paths are covered by the pure decision tests plus the manual mount check above.
