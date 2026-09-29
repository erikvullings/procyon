# 0234 Disk usage: macOS getattrlistbulk scanner

Status: done
Priority: low
Subsystem: backend
Depends on: 0230

## Context
BlitzTree reads each directory with `getattrlistbulk(2)`, which returns names, types, sizes,
link counts, flags and mount status for a whole batch of entries per syscall, instead of one
`lstat` per entry. It is ~40% faster than parallel `readdir` + `lstat` on APFS.

## Acceptance Criteria
- On macOS the local scanner lists directories with `getattrlistbulk`, preserving hardlink
  deduplication, unreadable reporting, cancellation and the 0230 boundaries.
- Other platforms keep the portable `read_dir` + `symlink_metadata` path and produce identical
  trees for the same fixture.
- A test compares both paths on a temp fixture on macOS.

## Agent Notes
- `fm-vfs-local::bulk_listing` (macOS only) wraps `getattrlistbulk` behind a safe API:
  `list_directory_bulk(path)` returns names plus optional `BulkAttributes` (kind, device,
  file id, BSD flags, link count, logical/physical bytes, mount-point flag). The unsafe code is
  confined to the open/syscall; entry parsing is pure, bounds-checked and unit-tested.
- `fm-application::disk_usage` lists through a `DirectoryLister` (`Portable` or `Bulk`, chosen
  by `DirectoryLister::native()`), producing `ListedEntry { name, info: Option<EntryInfo> }`.
  An entry without attributes (per-entry error, missing attribute, or a mount point, whose
  attributes describe the covered directory) falls back to `lstat`, so boundary and unreadable
  handling stay on one code path. `ScanBoundary::skip_reason` takes `&EntryInfo`.
- Hardlink identity uses `ATTR_CMN_FILEID`, which equals `st_ino` on APFS; on HFS+ the two
  could theoretically differ when bulk and `lstat` identities mix. Only APFS was tested.
- `bulk_lister_matches_the_portable_lister` compares both listers on a fixture with nested
  dirs, empty dir/file, unicode names, symlinks, a hardlink pair and a chmod-000 directory.
- Benchmark (release, `~/Library`, 1.09M nodes, identical output): portable 8.1 s warm vs
  bulk 6.1 s warm (~25% faster); on a cold cache 13.4 s vs 6.1 s.
