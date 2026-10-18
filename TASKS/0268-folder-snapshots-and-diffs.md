# 0268 Folder snapshots and diffs

Status: done
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
- Created 2026-10-18 from the media/playlist feature discussion. 
- 2026-10-18 claude: Format documented in docs/architecture/snapshot-format.md (JSON lines, version 1, optional SHA-256, tolerant of unknown fields, newer versions rejected; pure parse/render + comparison rules in crates/fm-application/src/snapshot_format.rs). Jobs (snapshots.rs; shared tree_walk.rs extracted from the link doctor): create writes via provider open_write with overwrite=false, cancellable, checksum progress; compare reads the snapshot, walks any target folder (verify a backup), reports added/removed/changed/corrupted (same size + mtime within 2 s but different SHA-256) and counts. Symlinks are never recorded/followed; the snapshot file itself is excluded. REST /api/v1/snapshot-jobs (+ GET/cancel; paths root-checked) + Tauri commands + client methods. UI: client.saveSnapshot (selected folder or active folder; file written next to the folder as <name>.procyon-snapshot, name/checksum dialog) and client.compareWithSnapshot (selected .procyon-snapshot vs the other pane's folder, else the active folder); results shown in the shared inline review panel rather than as a virtual folder. Comparison reuses the checksum engine (fm_checksum::hash_entry); the 0075 comparison engine's result model was not reused because it compares two trees, not a tree and a recorded file.
