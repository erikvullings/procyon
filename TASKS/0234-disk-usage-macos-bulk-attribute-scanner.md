# 0234 Disk usage: macOS getattrlistbulk scanner

Status: open
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
