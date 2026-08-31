# 0008 Cross-provider transfer planning

Status: done
Priority: high
Subsystem: backend
Depends on: 0004, 0006

## Context
Harden the operation engine for provider-specific fast paths and remote-to-remote streaming after SFTP and FTP exist.

## Acceptance Criteria
- Providers expose transfer capabilities such as server-side copy/move, resumable upload/download, and random read/write.
- Operation planning chooses safe provider-native operations when available.
- Otherwise it streams source → destination directly.
- `SFTP → FTP` and `FTP → SFTP` require no temporary local file.
- Progress remains provider-neutral.
- Cancellation reaches source and destination.
- Partial destination cleanup and conflict handling remain correct.
- Integration tests cover all supported local/SFTP/FTP direction pairs.

## Implementation Notes
- Add `TransferCapabilities` or equivalent.
- Keep strategy selection in the operation planner, not UI or individual commands.
- Test same-connection optimization separately from cross-provider streaming.

## Agent Notes
- Review the existing copy planner before changing provider interfaces; preserve local semantics.
- 2026-08-18 claude: Added `TransferCapabilities`/`TransferEndpoint` (`crates/fm-vfs/src/transfer.rs`)
  and a `FileSystemProvider::transfer_capabilities(&Location)` method. The key design decision is the
  opaque `TransferEndpoint`: `ProviderCapabilities` answers "can this provider type do X", but
  transfer planning needs "which concrete backend is this location on", and a `ProviderId`
  comparison cannot answer that — two `sftp://` locations may be different hosts. The default
  implementation derives the struct from `capabilities_for()` with the provider id as endpoint
  (correct only for single-backend providers), and `SftpFileSystemProvider`/`FtpFileSystemProvider`
  override it to return `sftp:<connection-id>` / `ftp|ftps:<connection-id>` so two saved connections
  never compare equal. Fields are `server_side_copy`, `server_side_move`, `resumable_upload`,
  `resumable_download`, `random_read`, `random_write` — the exact set the acceptance criteria name.
  `LocalFileSystemProvider` deliberately needs **no** override: endpoint `local`, and the derived
  flags (`server_side_copy`/`server_side_move`/`random_read` true) are exactly its existing
  behaviour, so local semantics are preserved by construction rather than by re-derivation.
- 2026-08-18 claude: Strategy selection lives entirely in `crates/fm-application/src/operation_planner.rs`
  as `TransferPlan::select(&TransferCapabilities, &TransferCapabilities) -> TransferPlan`
  (`TransferStrategy::{ServerSideCopy, DirectStream}`, `MoveStrategy::{ServerSideMove, CopyThenDelete}`,
  plus `same_endpoint`). It is a pure function of the two capability sets, resolved once in
  `OperationPlanner::plan()` for Copy/Move/Duplicate and stored on the `CopyExecutor`/`MoveExecutor`;
  execution only obeys it and never re-derives a strategy, so no UI path and no individual command
  can disagree. This replaced three separate `source_provider.id() == destination_provider.id()`
  checks (server-side-copy gating in `copy_file`, `preserve_metadata` in `CopyCommitOptions`, and the
  directory-timestamp pass in `CopyExecutor::finish`) plus `MoveExecutor::plan`'s `same_provider`
  check — every one of which was a provider-*type* comparison that would have wrongly qualified two
  different SFTP/FTP hosts as one backend. `MoveExecutor` still consults
  `FileSystemProvider::same_filesystem` after the strategy check, which is what keeps the local
  provider's cross-volume move semantics (rename fails across devices → copy+delete) bit-for-bit
  unchanged.
- 2026-08-18 claude: `SFTP → FTP` / `FTP → SFTP` needed no new plumbing to avoid a local temporary —
  the existing `copy_file` already opens the source's reader and the destination's writer and pumps
  between them, staging only in a `.fm-copy-<uuid>` file the *destination provider* owns. What was
  missing was proof, so the new integration tests assert it directly by walking the whole service
  root for any `.fm-copy-*` artifact. Cancellation was hardened: the streaming loop now captures its
  outcome instead of returning early, then explicitly drops the reader and awaits
  `writer.shutdown()` before propagating. Without that, a cancelled remote→remote transfer left the
  FTP provider's spawned `STOR` task still streaming in the background, racing the `cleanup_partial`
  that was supposed to discard the temporary.
- 2026-08-18 claude: Two real pre-existing bugs found and fixed while proving the direction pairs.
  (1) `fm_operations::safety::normalized_path` had branches for `archive` and `sftp` but **not**
  `ftp`, so every same-provider FTP location fell through to `Location::to_native_path()` and
  `validate_paths` returned `IncomparableLocations` — meaning **`FTP → FTP` copies and moves could
  never succeed at all**, on any path. Added an `ftp` branch that keeps the scheme in the comparison
  text (`ftp/<id>/...` vs `ftps/<id>/...`), because this workspace's FTP provider treats plain and
  TLS forms of one connection id as different endpoints and dropping the scheme would make them
  compare as the same entry. (2) `FtpFileSystemProvider::discard_copy` propagated `NotFound` when
  the temporary had never been created, which failed the *cleanup* of an already-cancelled
  operation; it now swallows `NotFound` like the local and SFTP providers already did.
- 2026-08-18 claude: Promoted an in-process FTP server from `fm-vfs-ftp`'s test file into
  `fm_vfs_ftp::fixture::FtpFixture` (mirroring the existing `fm_ssh::fixture` precedent) so
  `fm-application` integration tests can drive a real FTP wire protocol on loopback. It is a genuine
  improvement on the old test-local fixture, not a copy: `LIST` is now directory-scoped (reports
  only direct children of the requested path) rather than dumping every stored file regardless of
  directory, which is what makes nested FTP destinations testable. No test in this task reaches an
  external server.
- 2026-08-18 claude: Verified — **28 new tests**, all re-run by name:
  `crates/fm-application/tests/cross_provider_transfer.rs` **11 new**
  (`cargo test -p fm-application --test cross_provider_transfer`, 11 passed): `local → FTP`,
  `FTP → local`, `FTP → FTP` same connection, `FTP → FTP` two different connections, same-connection
  `FTP` move via server-native `RNFR`/`RNTO`, `SFTP → SFTP` two different connections, `SFTP → FTP`,
  `FTP → SFTP`, provider-neutral progress (asserts a cross-provider transfer's item/byte progress is
  *identical* to the same copy performed locally, i.e. that the strategy choice does not leak into
  progress), cross-provider conflict handling (waits for a decision, destination untouched, no stray
  temporary), and cancellation of `SFTP → FTP` (no published destination, no remote temporary, no
  local temporary). `crates/fm-application/src/operation_planner.rs` **8 new**
  (`cargo test -p fm-application --lib operation_planner`, 14 passed incl. 6 pre-existing) covering
  `TransferPlan::select`, including `every_direction_pair_across_five_backends_resolves_consistently`
  which walks the full 5×5 matrix of local/`sftp:a`/`sftp:b`/`ftp:x`/`ftps:x` in one scenario and
  asserts endpoint identity is symmetric, that exactly one pair qualifies for a server-side copy and
  exactly the five same-backend pairs qualify for a server-side move — a single-pair fixture would
  have passed even with an order-dependent or left-hand-biased implementation.
  `crates/fm-vfs/tests/provider_contract.rs` **3 new** (10 passed);
  `crates/fm-vfs-sftp/tests/provider.rs` **2 new** (21 passed);
  `crates/fm-vfs-ftp/tests/provider_contract.rs` **2 new** (8 passed, 2 pre-existing network tests
  still ignored); `crates/fm-vfs-local/tests/local_provider.rs` **1 new** (23 passed);
  `crates/fm-operations/tests/operation_engine.rs` **1 new** (17 passed).
  `cargo test --workspace`: 110 test binaries, 0 failures — in particular `copy_file_operation.rs`
  (9), `copy_directory_operation.rs` (7), `move_operation.rs`, `duplicate_operation.rs`,
  `conflict_resolution.rs` and `ssh_sftp_operations.rs` (10) all still pass unchanged, confirming
  local→local and the previously covered SFTP pairs did not regress. `cargo fmt --all --check` and
  `cargo clippy --workspace --all-targets -- -D warnings` both clean.
- 2026-08-18 claude: Known limitations, stated explicitly rather than left implicit.
  (a) `resumable_upload`/`resumable_download`/`random_write` are `false` for **every** provider in
  this workspace today. The fields exist and are honoured by the planner's inputs, but no provider
  implements offset-resumed transfers (SFTPv3 and FTP `REST` can both express them; neither
  `fm-vfs-sftp` nor `fm-vfs-ftp` does), and no provider exposes an offset-write API at all, so
  advertising them would make the planner select a path that cannot be executed. Under-advertising
  is the honest failure mode here, matching the reasoning already recorded in `fm-vfs-sftp`'s module
  documentation; a follow-up task that implements resumption should flip these flags and add the
  corresponding planner branch. Consequently no `TransferStrategy::ResumeUpload`-style variant
  exists — it would be dead code.
  (b) `server_side_copy` is therefore only ever `true` for the local provider, so the
  server-side-copy fast path is exercised end to end only for `local → local` (covered by the
  pre-existing `copy_file_operation.rs`); for SFTP/FTP the same-backend optimization that *is*
  reachable is the server-native move, which is tested for both.
  (c) `ROADMAP.md`'s rows for 0106 and 0109 were already stale (both are done); only the 0108 rows
  in `ROADMAP.md`/`TASKS/README.md` were updated, to stay scoped to this task.
