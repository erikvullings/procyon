# 0257 Split and combine files

Status: open
Priority: low
Subsystem: backend, frontend
Depends on: 0035, 0077

## Context
Total Commander File > Split/Combine file: split a large file into fixed-size parts (with a CRC/manifest) and re-join parts. Useful for FAT32 limits, uploads, and email. Not present today.

## Acceptance Criteria
- "Split file…" dialog: part size presets/custom, target folder, naming (`name.001`, …) and optional checksum manifest; runs as a cancellable job in `fm-operations`.
- "Combine files…" on `.001/.002…` selection (or one part): detects the set, verifies manifest if present, rejects gaps, joins into the target.
- Works across VFS providers via streaming (no full-file buffering); conflict policy via existing dialog (0045).
- Never overwrites silently; temp-root tests for destructive paths.

## Implementation Notes
- New operation kinds in `fm-operations`/`OperationsCoordinator`; reuse transfer planning for cross-provider targets.
- Manifest format documented in docs; keep simple (`.crc`/BLAKE3 list).

## Agent Notes
- Created 2026-10-18 from the feature gap review. No work started.
