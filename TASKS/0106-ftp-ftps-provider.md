# 0006 FTP and FTPS provider

Status: done
Priority: medium
Subsystem: backend
Depends on: 0003

## Context
Add FTP/FTPS as another `FileSystemProvider`. SFTP remains separate via 0004. Plain FTP must be clearly marked insecure.

## Acceptance Criteria
- Users can create FTP and FTPS connections.
- Passive transfers work.
- Listing, upload/download, mkdir, rename, supported move, and delete work.
- Explicit FTPS is supported and validates TLS certificates.
- Plain FTP is visibly identified as insecure.
- Provider capability reporting reflects FTP limitations.
- Cross-provider transfer uses the shared operation engine.
- Cancellation/partial cleanup work.
- Integration tests use isolated FTP/FTPS fixtures.

## Implementation Notes
- Suggested crate: `fm-vfs-ftp`.
- Evaluate a maintained Rust FTP/FTPS library such as `suppaftp`.
- Passive mode should be default.
- Do not fake watch/checksum/timestamp/permission/server-copy semantics.

## Agent Notes
- Keep protocol quirks in the provider and error mapper; avoid frontend protocol special cases.
- Implemented `fm-vfs-ftp` with passive FTP and explicit FTPS through `suppaftp`. FTPS uses the
  platform trust store and hostname verification; there is no certificate-bypass option.
- FTP/FTPS locations retain their transport scheme while sharing provider id `ftp`, so the common
  operation engine handles cross-provider streaming, temporary-copy commit, and partial cleanup.
- Capability reporting is intentionally limited to list/read/write/mkdir/rename/move/delete. The
  provider does not advertise watch, checksum, timestamps, permissions, trash, or server-side copy.
- Added an isolated in-process passive FTP fixture covering list/upload/download/mkdir/rename/delete,
  plus an explicit-FTPS fixture proving that an untrusted certificate is rejected.
- Added ignored, soft-failing live smoke tests for Rebex that list `/pub/example` and download
  `readme.txt` over both FTP and explicit FTPS. Set `FM_REBEX_STRICT=1` when running them to make
  third-party endpoint failures fail the test.
- Switched FTPS to rustls because Rebex requires TLS session reuse on protected data connections.
- Plain FTP is labelled `FTP (insecure)` in the connection editor with an explicit warning that
  credentials and files are sent without encryption.
- Verified with `pnpm test` and `pnpm run lint` on 2026-08-13.
