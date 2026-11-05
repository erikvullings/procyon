# 0309 Quick hash panel

Status: open
Priority: low
Subsystem: frontend, backend
Depends on: none

## Context
Checksums exist for duplicate detection (0077), but there's no fast "give me the hashes of these
files" tool — a daily need for verifying downloads, content-addressing, and integrity checks.

## Acceptance Criteria
- An action/panel that computes one or more digests (SHA-256 default; MD5, SHA-1, BLAKE3 optional)
  for a selection, streamed for large files, with copy-to-clipboard per algorithm.
- Optional "compare two selections / a selection against a pasted value" to verify equality.
- Uses the existing checksum capability (0077) over the VFS provider so it works on local + remote;
  progress + cancellation for multi-file/bulk jobs.
- Cross-host parity; never blocks the UI on a long hash.

## Implementation Notes
- Reuse `ChecksumCoordinator`/the checksum operation rather than a new hashing path; expose it as a
  bounded job in the operation centre.
- BLAKE3 only if the Rust dependency is already available; otherwise SHA-256/MD5/SHA-1 suffice.

## Agent Notes
- (none yet)
