# 0232 Disk usage: clean-up candidates

Status: open
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
