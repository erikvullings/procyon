# 0309 Quick hash panel

Status: done
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
- 2026-11-07 claude: most of this already existed from 0077 (job over the VFS, progress, cancel, copy,
  save, verify against a checksum file). Added what was missing: the shortcut action now computes
  every algorithm (SHA-256, BLAKE3, CRC-32, MD5) in one read pass, so the panel's algorithm picker
  and per-algorithm Copy are usable; and "Compare with a digest" (paste a value, matched
  case/whitespace-insensitively against every computed algorithm and file, pure
  `findDigestMatches` in `checksum-state.ts`). i18n in all 8 locales. SHA-1 is not offered (no
  backend support; spec lists it optional). Verified: 4 state tests + 1 view test, checksum/app
  vitest, tsc, biome. Limits: hashing four algorithms is somewhat slower than SHA-256 alone on very
  fast disks; not checked visually in a browser.
