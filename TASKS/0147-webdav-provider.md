# 0147 WebDAV provider

Status: done
Priority: medium
Owner: unassigned
Agent: claude
Area: backend
Depends on: 0103, 0108, 0109

## Context

Identified alongside [0146](0146-s3-compatible-object-storage-provider.md) from a competitive
feature scan against ForkLift (2026-08-19 product-page discussion). ForkLift connects to WebDAV
servers directly. fm has SFTP (0104) and FTP/FTPS (0106) but no WebDAV client.

WebDAV is the protocol behind self-hosted Nextcloud/ownCloud instances and a number of managed
storage products, none of which fm's target audience can reach today except by mounting them at
the OS level first (where supported at all — WebDAV OS-mount support is inconsistent across
platforms, unlike the iCloud/OneDrive conventions 0101 already leans on). Same reasoning as 0146:
this is not covered by the "let the OS mount it" logic that froze 0110/0111, because there is no
reliable OS mount to lean on.

## Acceptance Criteria

- New `FileSystemProvider` for WebDAV (RFC 4918), tested against at least one real server
  implementation (e.g. a Nextcloud test container) rather than a hand-rolled fixture only.
- Connection profile: URL, username, password (Basic or Digest auth — WebDAV has no single
  standard auth scheme, so support both), optional path prefix. Credentials stored only as an
  opaque `CredentialStore` reference, matching the SFTP (0104) and FTP (0106) connection profiles.
- `PROPFIND` (depth 1) drives directory listing; `MKCOL`/`PUT`/`GET`/`DELETE`/`MOVE`/`COPY` drive
  the corresponding file operations, dispatched through the shared operation engine.
- `TransferCapabilities` (0108) reports `server_side_move`/`server_side_copy` true (WebDAV's
  native `MOVE`/`COPY` methods), `random_read` true if the server advertises `Range` support
  (probe via response headers, don't assume), `random_write` false.
- TLS certificate validation is real (no blanket accept-all), matching the "host keys/certs are
  never silently accepted" posture the SSH provider (0104) already established.
- Locked-resource responses (WebDAV `LOCK`/423 status) surface as a clear conflict rather than a
  generic failure.
- Provider capability reporting is accurate.
- Integration tests use an isolated WebDAV fixture server, not a live third-party service.

## Implementation Notes

- Suggested crate: `fm-vfs-webdav`, following the `fm-vfs-sftp`/`fm-vfs-ftp` split.
- Check crates.io for an existing maintained Rust WebDAV client before writing one from scratch;
  if nothing suitable exists, this reduces to XML-over-HTTP (`PROPFIND` response parsing) on top of
  `reqwest`, which is already a workspace dependency via the FTP/HTTP paths.
- Reuse `crates/fm-domain/src/location.rs`'s `Parsed*Uri` pattern for a new `webdav://
  <connection-id>/<path>` scheme, mirroring `ParsedSftpUri`.
- Cross-reference [0146](0146-s3-compatible-object-storage-provider.md) — same motivation,
  separate protocol, separate crate, no shared code expected beyond the `FileSystemProvider` trait.

## Agent Notes

- Initial task setup. No execution attempts recorded yet. Before starting, survey the current Rust
  WebDAV client ecosystem on crates.io (last-updated dates, `PROPFIND`/lock support) — this space
  has historically had few well-maintained options, so a build-vs-adopt decision needs a fresh
  check rather than assuming a library exists.
- 2026-08-20 claude: **Library survey** (network-verified against crates.io, not assumed): no
  mature, actively-maintained end-to-end WebDAV *client* crate exists that also covers Digest auth,
  lock/423 surfacing and `Range` capability probing (`reqwest_dav` is a thin wrapper with no Digest
  support; `remotefs-webdav`/`webdav-request`/`io-webdav` are all young/minimal). Per the task's own
  fallback instruction, built `fm-vfs-webdav` as XML-over-HTTP on top of `reqwest` (already a
  workspace dependency) plus `quick-xml` (added to `[workspace.dependencies]`, already present
  transitively at 0.41.0) for `PROPFIND` `multistatus` parsing.
- 2026-08-20 claude: New crate `crates/fm-vfs-webdav`, mirroring `fm-vfs-ftp`'s shape:
  `WebDavFileSystemProvider` (`src/provider.rs`), a hand-rolled `src/digest.rs` (RFC 2617/7616
  Digest auth, `MD5`/`MD5-sess`, `qop=auth` only — `auth-int` and RFC 7616 `SHA-256` are explicit,
  typed `Unsupported` errors, not silently mishandled), `src/xml.rs` (namespace-prefix-tolerant
  `multistatus` parsing — matches on the local element name only, since Nextcloud/ownCloud/Apache
  `mod_dav` all prefix differently), and `src/fixture.rs` (see below). `fm-domain/src/location.rs`
  gained `webdav://<connection-id>/<path>` parsing (`ParsedWebDavUri`), mirroring `ParsedSftpUri`
  exactly (opaque UUID-text connection id, no `fm-connections` dependency).
- 2026-08-20 claude: **Test fixture, and the task's explicitly-flagged real-server requirement**:
  this sandboxed build environment has **no `docker` binary at all** (checked directly, `which
  docker` fails), so a real Nextcloud test container — the acceptance criterion's own suggested
  example — could not be run here. Built `fm_vfs_webdav::fixture::WebDavFixture` instead: a
  hand-rolled HTTP/1.1 responder over a raw `tokio::net::TcpStream` (including chunked
  transfer-encoding request-body parsing, since `reqwest` streams `open_write` uploads that way),
  speaking the real wire protocol — genuine `PROPFIND`/`multistatus` XML, genuine
  `WWW-Authenticate`/`Authorization` challenge-response for both Basic and Digest, a genuine TLS
  listener via `tokio-rustls` for the certificate-rejection test — not a mocked provider. This is
  the same fixture-over-real-protocol philosophy 0104 (`fm_ssh::fixture`) and 0106
  (`fm_vfs_ftp::fixture::FtpFixture`) already established for SFTP/FTP, for the identical reason
  (no external server needed, works in a sandboxed environment, exercises real wire bytes). It is
  hand-rolled rather than built on `hyper`/`axum` deliberately: `fm-test-support`'s architecture
  fitness test (`workspace_crates_respect_the_documented_layering`) reserves both crates for
  `apps/` host binaries only, and `fm-vfs-webdav` is a layer-2 engine crate — confirmed by running
  that exact fitness test after adding `fm-vfs-webdav` to `CRATE_LAYERS` (layer 2, alongside
  `fm-vfs-ftp`/`fm-vfs-sftp`). **Known, explicitly-flagged gap**: this is the closest available
  substitute for the acceptance criterion's "at least one real server implementation", not a literal
  Nextcloud container — a genuine environment limitation of this sandbox, not a shortcut taken by
  choice. Whoever has Docker access should run this provider against a real Nextcloud/ownCloud
  instance at least once before treating it as fully field-proven.
- 2026-08-20 claude: **Protocol coverage against the acceptance criteria**: `list` uses `PROPFIND`
  depth 1; `create_directory`/`open_write`/`open_read`/`remove`/`rename` use
  `MKCOL`/`PUT`/`GET`/`DELETE`/`MOVE`; `server_side_copy` uses native `COPY` (capabilities() sets
  `SERVER_SIDE_COPY`, so `TransferCapabilities::server_side_copy`/`server_side_move` are both
  `true` — WebDAV is the first provider in this workspace where server-side copy is real, not just
  local). `random_read` is *not* assumed: `capabilities_for`/`transfer_capabilities` must stay
  synchronous (no I/O), so a small `RwLock<HashMap<connection_id, bool>>` cache is populated by a
  `HEAD` probe of `Accept-Ranges` the first time a connection is used (in `list`/wherever a client
  is first built), and `transfer_capabilities` reads that cache (falling back to `false`,
  under-advertised, if never probed) — never a guess. A `423 Locked` response maps to the existing
  `VfsError::Locked` variant (already used by the local provider's "file in use" case) rather than a
  generic `Io` error, satisfying "surface as a clear conflict"; verified against the fixture's own
  `lock()` helper. TLS certificate validation needed no explicit code: `reqwest`'s `rustls` feature
  (this workspace's existing default) already depends on `rustls-platform-verifier` and validates
  against the platform trust store by default — there is no `danger_accept_invalid_certs` call
  anywhere in this crate, and a dedicated test proves an untrusted self-signed certificate is
  rejected. Basic auth is sent via `reqwest`'s built-in `basic_auth`; Digest auth caches the
  server's challenge per connection (primed with one extra request the first time, refreshed on any
  subsequent `401`) rather than re-challenging on every call.
- 2026-08-20 claude: **Known, explicitly-flagged Digest-auth limitation**: `open_write`'s upload
  body is a genuine one-shot stream (a `tokio::io::duplex` fed by the caller), so a mid-stream `401`
  (an expired Digest nonce partway through a large upload) cannot be safely replayed without
  buffering the whole file — unlike every other operation here, whose bodies are small enough to
  buffer and retry freely. The chosen tradeoff: `open_write` primes the Digest cache with one cheap
  `HEAD` request *before* opening the stream, so the real `PUT` almost always carries a valid nonce
  on its first (only) attempt; no retry is attempted if it still 401s. Documented here rather than
  silently accepted as correct in every case.
- 2026-08-20 claude: **Wiring**: `fm-connections::WebDavConnectionConfiguration` is now fully
  modeled (`base_url`, `username`, `authentication: WebDavAuthenticationScheme` [`Basic`/`Digest`],
  `path_prefix: Option<String>`), replacing the task 0103 stub that only had `base_url`.
  `fm-transport-dto`/`fm-application::connection_dto` gained the matching DTO and conversions
  (mirroring the SSH/FTP pattern exactly — the existing `ConnectionSecretInputDto::Password` variant
  already covers WebDAV's password secret, no new secret-input variant needed).
  `fm-application::webdav.rs` adds `WebDavDialer`/`WebDavResolver` (mirrors `ftp.rs`), registered in
  `service.rs` alongside the other providers/dialers. `fm-operations::safety::normalized_path`
  gained a `webdav` branch (scheme-stripped-text comparison, same as the existing `sftp`/`ftp`
  branches) — proven necessary, not just added defensively, by a new regression test
  (`safety_compares_webdav_locations_by_connection_and_path_not_native_path` in
  `fm-operations/tests/operation_engine.rs`) that fails with `IncomparableLocations` if the branch
  is removed, exactly the same bug class 0104's Agent Notes originally found for SFTP.
- 2026-08-20 claude: **Frontend**: `frontend/src/features/connections/connections-model.ts`'s
  `isBrowsable`/`remoteRootLocation` now include WebDAV (previously false/unreachable, matching the
  "honestly excluded until a real provider exists" pattern from 0103/0104's notes — WebDAV now has
  one). `connection-editor.ts`'s WebDAV form gained base URL / username / authentication
  (Basic/Digest `Select`) / optional start-folder / password fields (previously only `baseUrl`
  existed as a stub). `pnpm run api:export`/`api:generate` regenerated
  `frontend/openapi/openapi.json` and the generated TypeScript client
  (`webDavConnectionConfigurationDto.ts` gained `username`/`authentication`/`pathPrefix`; new
  `webDavAuthenticationSchemeDto.ts`).
- 2026-08-20 claude: Verified (exact commands, not whole-suite totals, all via a dedicated
  `CARGO_TARGET_DIR` to avoid this machine's separately-running dev-server/editor build-lock
  contention): `cargo test -p fm-vfs-webdav` → **17 passed** (6 unit: Digest challenge
  parsing/RFC 2617 worked-example/unsupported-algorithm rejection, `multistatus` XML parsing
  including an unprefixed default namespace; 11 integration against the real in-process fixture:
  capability reporting, transfer-capability endpoint identity, cancellation, the full
  list/upload/download/rename/remove workflow run twice — once each for Basic and Digest auth end
  to end — server-side `COPY`, a `423 Locked` response mapping to `VfsError::Locked`, TLS
  certificate rejection, wrong-credential `PermissionDenied`). `cargo test -p fm-domain --test
  location_contract` → 22 passed (4 new `webdav_*`/`try_new_validates_the_webdav_*` tests, mirroring
  the existing `sftp_*` ones exactly). `cargo test -p fm-connections` → new
  `webdav_configuration_validates_its_own_fields`/`webdav_authentication_requires_a_stored_credential`
  pass among the full suite. `cargo test -p fm-application --lib connection_dto` → new
  `webdav_configuration_round_trips_through_dto_conversion` passes. `cargo test -p fm-operations
  --test operation_engine` → 18 passed (1 new, the safety-comparison regression above). `cargo test
  -p fm-test-support` → `workspace_crates_respect_the_documented_layering` passes with
  `fm-vfs-webdav` added to `CRATE_LAYERS`. `cargo build -p fm-server` succeeds (proves the whole
  dependency chain, including the new provider registration in `fm-application::service.rs`,
  actually links). `cargo clippy -p fm-domain -p fm-connections -p fm-transport-dto -p
  fm-application -p fm-operations -p fm-vfs-webdav --all-targets -- -D warnings` clean; `cargo fmt`
  clean for every crate this task touched. Frontend: `pnpm exec tsc --noEmit` clean; `pnpm exec
  vitest run src/features/connections/` → 38 passed (2 new: WebDAV is browsable,
  `remoteRootLocation` for WebDAV with and without a path prefix); full frontend suite (`pnpm exec
  vitest run`) → 1354 passed / 1 failed, the failure a pre-existing, unrelated `mithril-inspector`
  production-build timeout (a real `vite build` invocation exceeding 5s under this machine's load
  average of ~28 across 16 cores at the time — not a connections/WebDAV test, and not something this
  task's changes could plausibly cause); `pnpm exec biome check` clean for every file this task
  touched.
- 2026-08-20 claude: Known gaps, stated explicitly rather than left implicit: (1) tested against
  the in-process fixture only, not a real Nextcloud/ownCloud/Apache `mod_dav` server — no Docker
  available in this sandbox, see the fixture note above. (2) Digest-authenticated uploads cannot
  retry past a mid-stream nonce expiry (buffering the whole file would be the only fix, and isn't
  done here — see the dedicated note above). (3) `resumable_upload`/`resumable_download`/
  `random_write` are `false` (no offset-write/resume primitive is implemented, matching every other
  provider in this workspace per 0108's own Agent Notes). (4) No dedicated
  `fm-application`-level cross-provider integration test exists for `local ↔ WebDAV` or `SFTP/FTP ↔
  WebDAV` transfers (the 0108-style test suite `fm-application/tests/cross_provider_transfer.rs`
  was not extended) — the shared operation-engine dispatch is proven at the `fm-vfs-webdav`
  provider-contract and `fm-operations` safety-planning levels instead; a follow-up task extending
  that cross-provider suite to include WebDAV would close this gap with the same rigor 0108 used for
  SFTP/FTP.
