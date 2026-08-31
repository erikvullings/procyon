# 0155 Add unit-level coverage for S3 provider multipart upload paths

Status: done
Priority: low
Subsystem: backend
Depends on: none

## Context

Found via `/improve-codebase-architecture`. `crates/fm-vfs-s3/src/provider.rs` (1,018 lines, 3
`pub fn`) threads multipart upload state (`drive_upload`, `multipart_upload`, `upload_parts`,
`put_object`, `copy_object`) through `open_write`/`commit_copy`/`discard_copy` with no unit-level
tests — coverage today is entirely black-box integration tests against a fixture server (562
lines, in a separate file). Retry and partial-failure paths for multipart uploads aren't exercised
independently of the fixture server.

Weaker/lower-confidence finding than 0152–0154: integration tests do exist and exercise this code,
so this is a coverage gap rather than an active architectural friction point. Worth a look if the
S3 provider (task 0146) gets more scrutiny later, but not urgent.

## Acceptance Criteria
- Retry and partial-failure paths in the multipart upload flow (`drive_upload`,
  `multipart_upload`, `upload_parts`) have unit-level test coverage that doesn't require the fixture
  server, if the current implementation shape allows isolating those paths without a large refactor.
- If isolating this logic for unit testing requires restructuring `provider.rs` first, note that as
  a prerequisite rather than forcing tests onto the current shape.
- Existing integration test suite for `fm-vfs-s3` continues to pass unchanged.

## Implementation Notes
- First determine whether `drive_upload`/`multipart_upload`/`upload_parts` can be tested against a
  mocked S3 client trait without disturbing the provider's public interface — if the S3 SDK client
  is already behind an internal trait/seam, this may be a small addition; if not, decide whether
  introducing one is worth it for this specific gap before doing so.

## Agent Notes
- 2026-08-25: Task created from `/improve-codebase-architecture` findings (candidate 5, flagged as
  the weakest of the set during initial exploration). Not yet investigated further.
- 2026-08-28: Added a private `MultipartUploadClient` seam without changing
  `S3FileSystemProvider`'s public interface. Part uploads now make at most three attempts for
  transport failures, HTTP 408/429, and 5xx responses; permanent failures stop immediately.
  Exhausted, later-part, and completion failures all abort the multipart upload. Retry request
  bodies use reference-counted `Bytes`, preserving the provider's bounded-memory behavior.
  Added 7 unit tests in `provider.rs` covering `drive_upload` single/multipart dispatch, transient
  retry recovery, permanent failure, retry exhaustion, later-part failure, completion failure,
  and abort behavior without the fixture server. Verified with `cargo test -p fm-vfs-s3 --lib`
  (7/7 unit tests), `cargo test -p fm-vfs-s3` (7 unit and 18 unchanged integration tests passed;
  1 real-endpoint smoke test ignored), `cargo clippy -p fm-vfs-s3 --all-targets -- -D warnings`,
  `pnpm run lint`, and the full `pnpm test` suite.
