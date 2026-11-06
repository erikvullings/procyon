# 0316 WebDAV transfer resume

Status: open
Priority: low
Subsystem: backend, remote
Depends on: 0303

## Context
Split from 0303. WebDAV downloads can continue with a ranged `GET` (`Range` / `If-Range`); a
plain WebDAV `PUT` cannot be appended to, and there is no standard partial-upload resume (some
servers implement SabreDAV's `X-Update-Range` or Nextcloud chunked uploads, each different).

## Acceptance Criteria
- `read_range` is implemented with `Range` requests, validated with `If-Range`/ETag so a source
  that changed mid-transfer is detected, and `random_read` is advertised only when the server
  answers `206 Partial Content`.
- Resumable DOWNLOAD (WebDAV source to local or SFTP destination) continues a private partial
  using the existing engine proof.
- Resumable UPLOAD is implemented only for servers that provably support it (capability probe
  per server flavour); every other server keeps restarting and says so.
- Fault-injection tests against a fake WebDAV server: server ignores `Range`, source ETag
  changes, truncated partial, expired credentials.

## Implementation Notes
- The engine currently resumes only uploads (it appends to a destination partial). A download
  resume is the same operation seen from the other side: the destination is local and
  appendable, the source supports `read_range`; the engine path already reads the source with
  `read_range`, so most of the work is in the provider.

## Agent Notes
- 2026-11-06 claude: Split from 0303, which delivered the shared machinery and SFTP only.
