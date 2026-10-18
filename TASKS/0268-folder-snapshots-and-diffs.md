# 0268 Folder snapshots and diffs

Status: open
Priority: low
Subsystem: backend, frontend
Depends on: 0077, 0075

## Context
Record a folder's state (paths, sizes, mtimes, optional checksums) and later compare against it to see what changed or silently corrupted — useful for backups and archives.

## Acceptance Criteria
- Action 'Save snapshot…' writes a compact snapshot file (documented, versioned format) for a folder tree, optionally with checksums, as a cancellable job.
- 'Compare with snapshot…' shows added/removed/changed/corrupted (same mtime and size but different checksum) entries as a virtual folder.
- Snapshots can be compared against a different location (verify a backup copy).

## Implementation Notes
- Reuse the comparison engine (0075) result model and checksum engine (0077).

## Agent Notes
- Created 2026-10-18 from the media/playlist feature discussion. No work started.
