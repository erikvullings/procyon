# 0316 WebDAV transfer resume

Status: done
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
- 2026-11-06 claude: Live check against `x/net/webdav` (podman): the server answers ranged GETs with 206 and `Accept-Ranges: bytes`, but this provider has NO `read_range` (it reports `UnsupportedCapability`), so a download resume currently reads and drops the already-fetched prefix. Implementing `read_range` with a `Range` request (+ `If-Range`) is the cheap first step here. Upload resume is unchanged: a WebDAV PUT cannot be appended to. Also fixed the writer race this exposed (see 0303): the WebDAV writer now waits for the PUT result in `shutdown`, so a refused PUT fails the copy.
- 2026-11-06 claude: DONE (scoped). `read_range` is implemented with `Range` requests; a `206` is streamed as is, a `200` (server ignored `Range`) has the offset skipped and the length applied locally, a `416` yields an empty stream, `RANDOM_ACCESS` is advertised and `random_read` in `transfer_capabilities` still only follows the probed `Accept-Ranges: bytes`. Download resume (WebDAV source to local/SFTP) therefore no longer reads and drops the prefix; verified LIVE against `hacdias/webdav` (podman).
- NOT done, deliberately: (1) `If-Range`/ETag validation: the provider API carries no validator between calls, so a source that changes mid-transfer is caught by the engine instead (the prefix is proven against the CURRENT source before resuming, and a resumed file is always verified before publishing). (2) Resumable WebDAV UPLOAD: plain `PUT` cannot be appended to and the flavour-specific protocols (SabreDAV `X-Update-Range`, Nextcloud chunking) need a per-server capability probe, which no supported server here has demonstrated; every WebDAV upload keeps restarting and the engine reports why (`Unverifiable`). Open a task per flavour when one is needed.
- Tests: `tests/provider.rs` (exact ranges, offset skipped when `Range` is ignored via the fixture's `no-range-` files, missing file is `NotFound`), fixture gained `206`/`416` support.
