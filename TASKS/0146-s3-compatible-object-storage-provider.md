# 0146 S3-compatible object storage provider

Status: done
Priority: medium
Owner: unassigned
Agent: unassigned
Area: backend
Depends on: 0103, 0108, 0109

## Context

Identified from a competitive feature scan against ForkLift (2026-08-19 product-page discussion).
ForkLift connects directly to Amazon S3, Backblaze B2, Rackspace CloudFiles and other S3-API
buckets as first-class remote locations. fm has SFTP (0104) and FTP/FTPS (0106), but nothing that
speaks the S3 API.

This is a genuinely different case from the frozen OneDrive/SMB providers (0110/0111): those are
parked because the OS already mounts them as regular folders, so a bespoke API client buys little.
Object storage buckets (S3, Backblaze B2, Cloudflare R2, DigitalOcean Spaces, MinIO — all speak the
same S3-compatible API) have **no** OS-native mount without a third-party tool (e.g. Mountain Duck,
`s3fs`), so "let the OS mount it" doesn't apply here. Given fm's existing audience — SSH/SFTP,
keyboard-first, dev/ops-leaning — buckets are plausibly a more relevant remote target than the
frozen consumer-cloud providers.

## Acceptance Criteria

- New `FileSystemProvider` for S3-compatible object storage (works against real AWS S3 and at
  least one non-AWS S3-compatible endpoint — e.g. MinIO or Cloudflare R2 — via a configurable
  endpoint URL, not hardcoded to `amazonaws.com`).
- Connection profile: access key id, secret access key, region, endpoint URL (optional, defaults
  to AWS), bucket, and optional path prefix. Credentials stored only as an opaque `CredentialStore`
  reference (macOS Keychain / Windows Credential Manager / in-memory fallback), matching the SFTP
  (0104) and FTP (0106) connection profiles — never embedded in the `Location` URI.
- Buckets browse like folders: prefix-based "directory" listing (S3 has no real directories — keys
  with a shared `/`-delimited prefix behave as one), paged, with `ListObjectsV2` delimiter/prefix
  semantics.
- Upload, download, delete, and rename-via-copy-then-delete (S3 has no native rename) work through
  the shared operation engine.
- `TransferCapabilities` (0108) reports what S3 actually supports: no `server_side_move` (no native
  rename), `server_side_copy` true only within the same bucket/region if using S3's native
  server-side `CopyObject`, `random_read` true (ranged GET), `random_write` false.
- Multipart upload for files above S3's single-PUT limit (5 GiB) or above a configurable threshold,
  so large-file transfers don't buffer the whole file in memory.
- Provider capability reporting is accurate (no directories, no in-place rename, etc. — the UI
  should not offer operations the provider can't actually do).
- Integration tests run against a local S3-compatible fixture (e.g. MinIO in a test container, or
  an in-process mock server) rather than requiring real AWS credentials in CI.

## Implementation Notes

- Suggested crate: `fm-vfs-s3`, following the `fm-vfs-sftp`/`fm-vfs-ftp` split (a provider crate
  implementing `FileSystemProvider` from `fm-vfs`, plus a thin transport layer).
- The AWS Rust SDK (`aws-sdk-s3`) works against any S3-compatible endpoint when given a custom
  endpoint URL and path-style addressing; weigh it against a lighter presigned-request client (e.g.
  `rusty-s3`) — the SDK pulls in a large dependency tree for what is fundamentally a handful of
  signed HTTP calls, and this workspace's other VFS providers (`fm-vfs-sftp`, `fm-vfs-ftp`) are
  comparatively lean. Check crates.io directly for current maintenance status before picking either
  (per 0104's own precedent of verifying library choices against the live registry, not assumption).
- Reuse `crates/fm-domain/src/location.rs`'s `Parsed*Uri` pattern for a new `s3://<connection-id>/
  <key-prefix>` scheme, mirroring `ParsedSftpUri`.
- No real filesystem "directory" exists — `mkdir` should either no-op (a prefix isn't a real
  object) or create a zero-byte marker object, matching what most S3 clients do. Decide and
  document the choice rather than silently picking one.
- Cross-reference [0147](0147-webdav-provider.md) — same "remote-provider breadth" motivation,
  separate protocol, separate crate. Land independently; no shared code expected beyond the
  `FileSystemProvider` trait itself.

## Agent Notes

- Initial task setup. No execution attempts recorded yet. Before starting, confirm the SDK/library
  choice (aws-sdk-s3 vs. a lighter presigned client) and validate the MinIO-based test fixture
  approach works in this project's CI sandbox before writing provider code against it.
- 2026-08-19: Implemented end to end with TDD. Library choice: checked crates.io directly (per
  0104's precedent) - `rusty-s3` 0.10.2 (updated 2026-08-01, Sans-IO presigned-request signer, pairs
  with the workspace's existing `reqwest` dep) over `aws-sdk-s3` 1.142.0 (actively maintained but a
  much heavier dependency tree for what is fundamentally a handful of signed HTTP calls, matching
  the lean style of `fm-vfs-ftp`/`fm-vfs-sftp`). `rusty-s3` has no dedicated `CopyObject` action;
  `CopyObject` is implemented as `PutObject` + an `x-amz-copy-source` header via
  `S3Action::headers_mut()`, matching the real S3 wire protocol.
  - New crate `crates/fm-vfs-s3` (`fm-vfs-s3`), layer 2, mirroring `fm-vfs-ftp`/`fm-vfs-sftp`:
    `S3FileSystemProvider` (`src/provider.rs`), the `S3ConnectionResolver`/`S3ConnectionParameters`
    seam (`src/resolver.rs`, deliberately never depends on `fm-connections`/`fm-credentials`, same
    rationale as `fm-vfs-sftp`'s `SshConnectionResolver`), and an in-process mock S3-compatible HTTP
    server (`pub mod fixture`, `S3Fixture`) implementing enough of the REST API (path-style
    bucket/key routing, `ListObjectsV2` prefix/delimiter, ranged `GetObject`, `PutObject`,
    `CopyObject`, `DeleteObject`, and the multipart trio) to exercise the provider without Docker or
    real AWS credentials - it does not verify SigV4 signatures, matching the "hand-rolled fake
    server" convention `fm-vfs-ftp`'s and `fm-ssh`'s fixtures already use rather than testcontainers
    (this repo has no docker-compose/testcontainers anywhere, and the CI matrix runs macOS/Windows
    runners that can't host a MinIO service container anyway).
  - `mkdir` creates a zero-byte marker object whose key ends in `/` (documented choice, matching
    most other S3 clients/the AWS console), rather than a silent no-op.
  - `rename`/`commit_copy` do `CopyObject` then `DeleteObject` (no native rename).
    `server_side_copy` does a real `CopyObject` within one connection/bucket; cross-connection
    returns `Ok(false)` so the caller falls back to streaming rather than erroring.
    `TransferCapabilities`: `server_side_move: false`, `server_side_copy: true`, `random_read: true`
    (ranged `GetObject`, implemented via `read_range`), `random_write: false`.
  - `remove`: real S3 `DELETE` is idempotent (204 whether or not the key existed, and never says
    which) so the provider can't tell a file key from its own directory-marker key apart from the
    delete result; `remove` therefore deletes both `key` and `key/` unconditionally rather than
    trying one and inspecting the response - documented in a doc comment on `remove`.
  - Multipart upload: `open_write` buffers up to a configurable threshold
    (`DEFAULT_MULTIPART_THRESHOLD` = 64 MiB, always well under S3's 5 GiB single-`PUT` limit) before
    deciding between one `PutObject` and a `CreateMultipartUpload`/`UploadPart`/
    `CompleteMultipartUpload` sequence, so an upload of unknown total length never buffers more than
    the threshold at once; a failed multipart upload calls `AbortMultipartUpload`.
  - `fm-domain/src/location.rs`: added the `s3` scheme (`SCHEME_MAP`) and `ParsedS3Uri` mirroring
    `ParsedSftpUri` exactly, wired into `Location::parse`/`parent`/`join`/`name`. Along the way, fixed
    a latent bug in `parse_scheme`: it rejected digits anywhere in a URI scheme (RFC 3986 permits
    `ALPHA *(ALPHA/DIGIT/"+"/"-"/".")` after the first character), which made `"s3"` itself
    unparseable - `s3_locations_*` tests in `location_contract.rs` caught this immediately.
  - `fm-credentials`: added `SecretMaterial::AccessKey { access_key_id, secret_access_key }` (access
    key id as a plain `String`, matching `PrivateKeyPath::path`'s "not secret" precedent; only the
    secret key is `Zeroizing`), plus `codec.rs` encode/decode round-trip support.
  - `fm-connections`: extended the existing `S3ConnectionConfiguration` stub with `access_key_id`
    and `start_path` (an initial-browse hint, same non-enforced semantics as
    `SshConnectionConfiguration::start_path` - the provider itself does not special-case it, matching
    precedent), added `EmptyS3AccessKeyId` validation, and flipped `requires_stored_credential` to
    `true` for `S3`. `fm-transport-dto`/`fm-application::connection_dto` updated in lockstep,
    including a new `ConnectionSecretInputDto::AccessKey` variant.
  - `fm-application`: new `crates/fm-application/src/s3.rs` (`S3Dialer` + `S3Resolver`, mirroring
    `ftp.rs`), registered in `service.rs` alongside the other providers/dialers.
  - `fm-test-support/src/architecture.rs`: added `fm-vfs-s3` to `CRATE_LAYERS` at layer 2.
  - Docs: `README.md` (feature bullet, crate tree, the "Remote & cloud" narrative paragraph) and
    `docs/architecture/filesystem-watching.md` updated for the new provider's polling-based
    `ChangeTracking`.
  - Verified: `cargo test -p fm-vfs-s3` - 18/18 new contract tests pass (upload/list/download,
    nested directories via `mkdir`, overwrite guard, ranged read, rename, recursive remove,
    multipart above a forced low threshold, `same_filesystem`, `watch` unsupported, `transfer_capabilities`
    across two distinct connections, `server_side_copy` + `commit_copy`, `discard_copy` of a
    never-created temporary), all against `S3Fixture`, no real credentials or Docker. `cargo test -p
    fm-credentials` (28 passed, incl. 3 new for `AccessKey`), `cargo test -p fm-domain` (18 passed in
    `location_contract`, incl. 4 new for the `s3` scheme), `cargo test -p fm-connections -p
    fm-transport-dto -p fm-application` (106 passed for the first two combined, 244 for
    `fm-application` including its `conflict_resolution` integration suite - unrelated to this
    change, just slow), `cargo test -p fm-test-support` (architecture fitness test passes with
    `fm-vfs-s3` correctly layered). `cargo clippy --all-targets` clean (zero warnings) on every
    touched crate; two `collapsible_if` warnings in the fixture were fixed inline.
  - Known gaps, honestly flagged rather than silently skipped: not validated against real AWS S3
    (see below for real MinIO validation, which did happen). No frontend/UI work was done - the
    task's acceptance criteria are entirely backend/provider-scoped, and the frontend connection
    editor had no S3-specific code before this change either, so wiring an S3 connection form is
    left as an explicit follow-up. No end-to-end test exercises the S3 provider through
    `fm-application`'s `OperationPlanner`/executor (only direct provider-level contract tests) -
    this matches `fm-vfs-ftp`/`fm-vfs-sftp`'s own test structure, which also test the provider
    directly rather than through the full operation engine.
- 2026-08-20: Validated against a real local MinIO instance
  (`brew install minio/stable/minio minio/stable/mc`; a `.minio-data`-backed server on
  `127.0.0.1:9000` with a `fm-test-bucket` capped at a 10 MiB hard quota via `mc quota set`), per
  the user's request for real-endpoint coverage beyond the mock fixture. This immediately caught
  two real bugs the mock fixture's lack of SigV4 verification had been masking:
  - `read_range` signed the presigned `GetObject`/`Range` request with the header name capitalized
    (`"Range"`); SigV4 canonicalization requires signed header *names* to be lowercase, so a real
    signature-verifying endpoint rejected every ranged read with 403 `PermissionDenied` (the
    fixture, which never checks signatures, had no way to catch this). Fixed by signing/sending
    `"range"` lowercase, matching `x-amz-copy-source`'s (already-correct) lowercase convention in
    `copy_object`.
  - `open_write`'s buffer-then-decide multipart strategy could hand S3 a first part smaller than
    its hard 5 MiB minimum non-final-part size, which a real endpoint rejects with `EntityTooSmall`
    (again invisible to the fixture, which imposes no minimum). Fixed by clamping
    `multipart_threshold` up to a new `MINIMUM_MULTIPART_PART_SIZE` (5 MiB) constant in
    `S3FileSystemProvider::with_multipart_threshold`, and updated both the mock-fixture multipart
    test and a new real-endpoint smoke test to use payloads large enough to actually exercise the
    multipart path post-clamp.
  - Added `real_endpoint_smoke_test` (`#[ignore]`d by default, `cargo test -p fm-vfs-s3 --test
    provider_contract -- --ignored real_endpoint_smoke_test`) to `tests/provider_contract.rs`:
    upload/download/list, rename (real `CopyObject`+`DeleteObject`), and a forced multipart upload,
    all against a real endpoint via `FM_S3_SMOKE_ENDPOINT`/`FM_S3_SMOKE_BUCKET`/`FM_S3_SMOKE_REGION`/
    `FM_S3_SMOKE_ACCESS_KEY_ID`/`FM_S3_SMOKE_SECRET_ACCESS_KEY` env vars, defaulting to the local
    MinIO setup above. Unlike `fm-vfs-ftp`'s public-server smoke test it hard-fails rather than
    soft-skipping (it targets a server the caller just started on purpose), and it cleans up every
    object it creates so repeat runs never grow the bucket. Verified: full run against local MinIO
    passes; `cargo test -p fm-vfs-s3` (18/18 mock-fixture tests still pass, real-endpoint test
    correctly `ignored` by default) and `cargo clippy -p fm-vfs-s3 --all-targets` (clean) re-run
    after the fixes. Still not validated against real AWS S3 itself or a non-MinIO S3-compatible
    endpoint (R2/B2) - the local MinIO instance is the `.minio-data`/`fm-test-bucket` setup above,
    left running for any follow-up testing; not currently wired into CI.
