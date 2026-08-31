# 0004 SFTP provider

Status: done
Priority: high
Subsystem: backend
Depends on: 0003

## Context
Add SSH-based file management via SFTP as a new `FileSystemProvider`. The product may call this SSH/SFTP, but legacy SCP is not the primary implementation.

## Acceptance Criteria
- SSH connections support host, port, username, and initial auth methods.
- SSH host keys are verified, first use is confirmable/persisted, and changed keys are never silently accepted.
- SFTP locations open in either pane.
- Listing, metadata, mkdir, rename, upload, download, supported moves, and delete work.
- `local → SFTP`, `SFTP → local`, and same-connection SFTP transfers use the shared operation engine.
- Cancellation and partial-file cleanup work.
- Provider capability reporting is accurate.
- No credentials are embedded in `Location` URIs.
- Integration tests use an isolated SSH/SFTP fixture.

## Implementation Notes
- Suggested crates: `fm-ssh`, `fm-vfs-sftp`.
- Evaluate current async Rust SSH/SFTP libraries such as `russh`/`russh-sftp`.
- Prefer locations referencing `ConnectionId`.
- Start with password/private-key auth; agent/jump-host/resume can follow.
- Keep recursive copy semantics in the operation engine.

## Agent Notes
- Validate current VFS stream interfaces before coding so remote reads/writes plug into existing transfer planning.
- 2026-08-09: Implemented end to end with TDD. New crates: `crates/fm-ssh` (session/authentication/host-key
  logic - connection-agnostic, deliberately never depends on `fm-connections`/`fm-credentials`, see
  design decision below) and `crates/fm-vfs-sftp` (the `SftpFileSystemProvider` `FileSystemProvider`
  implementation). `fm-domain/src/location.rs` gained real `sftp://<connection-id>/<path>` parsing
  (`ParsedSftpUri`, mirroring `ParsedFileUri`/`ParsedArchiveUri`'s style exactly): removed `"sftp"`
  from `RESERVED_SCHEMES` (now empty, kept as the seam a future task, e.g. FTP, can reserve its own
  scheme ahead of implementing it), added `SFTP_PROVIDER`/`SFTP_SCHEME` and full `join`/`parent`/`name`
  support. The connection id is validated as UUID text via the `uuid` crate directly (already a
  `fm-domain` dependency), not via `fm_connections::ConnectionId` - `fm-domain` must not depend on
  `fm-connections`.
- 2026-08-09: **Library choice**: `russh` 0.62.5 + `russh-sftp` 2.4.0 (both added to
  `[workspace.dependencies]`), plus `rand` 0.10 for in-process key generation. Chosen after checking
  crates.io directly (network-verified, not assumed) and confirming both compile and work together on
  this toolchain (Rust 1.97.1, edition 2024); `russh-sftp` itself lists `russh` as a dev-dependency,
  confirming the pairing is actively tested upstream. Both support client *and* server roles for the
  same wire protocol, which drove the fixture choice below. Auth: password via
  `Handle::authenticate_password`; private key via `russh::keys::decode_secret_key(pem_text, passphrase)`
  (parses PEM text directly, never a file path - credentials stay in memory only) +
  `authenticate_publickey`. SSH agent auth is a named, explicit gap (`SshError::UnsupportedAuthenticationMethod`),
  matching the task's own "start with password/private-key; agent can follow" note.
- 2026-08-09: **Test fixture choice**: an in-process `russh`/`russh-sftp` **server** (`fm_ssh::fixture`,
  unconditionally `pub`, not `#[cfg(test)]`, so `fm-vfs-sftp` and `fm-application` can depend on it too),
  over spawning the system `sshd`/`sftp-server` (task's option (b)). Chosen because it needs no external
  process or privileged config file (works identically in a sandboxed environment), and because it is a
  genuinely independent implementation from the client code under test (real `SSH_FXP_*` wire packets,
  real KEX/auth negotiation, real ed25519 host + client keys generated fresh per fixture via
  `PrivateKey::random`) - an actual protocol round trip, not a mock. The fixture serves a real temporary
  directory using the client-presented path as-is (no virtual-root translation), so tests address remote
  files by joining real paths under `fixture.root`.
- 2026-08-09: **Host-key confirmation design** (spec §6.4, the task's flagged open design fork):
  - `fm_ssh::KnownHostsStore` (`JsonFileKnownHostsStore`, one JSON file, atomic temp-file-then-rename
    writes) persists `{fingerprint, accepted_at}` keyed by an opaque caller-chosen string - documented
    choice: keyed by `ConnectionId` text, not host:port, so editing a connection's host/port fails
    closed (reverifies from scratch) rather than silently reusing a stale trust decision.
  - `fm_ssh::verify_host_key` compares a freshly presented fingerprint against the store, returning
    `Trusted`/`Unverified`/`Mismatch` - a pure function, never itself mutating the store. `SshSession::connect`'s
    `client::Handler::check_server_key` calls it *before* any credential is sent; `Unverified`/`Mismatch`
    reject the handshake and the caller-visible error is `SshError::HostKeyUnverified{fingerprint}` /
    `SshError::HostKeyMismatch{fingerprint, expected_fingerprint}` - two distinct variants, verified by
    dedicated tests, so a caller can never confuse "never seen" with "changed" or with a wrong
    password/network failure (which fall through to `SshError::Connect`/`AuthenticationFailed`).
  - `SshSession::probe_host_key`/`SshConnectionManager::probe_host_key` connect and verify the host key
    *without* authenticating, then disconnect - lets a caller ask "what is this host presenting right
    now" before it necessarily has a working credential, sharing the exact same verification code path
    as a real `connect` (refactored into a shared `establish_transport` helper) so the probe answer is
    never a separate, potentially-diverging code path.
  - The only way a fingerprint is ever persisted is `KnownHostsStore::accept` - `fm-application`'s new
    `FileManagerService::accept_ssh_host_key(connection_id, fingerprint)` facade method re-probes the
    host itself and refuses to persist if the presented fingerprint no longer matches the one being
    accepted (defense against confirming a stale/attacker-supplied value), and additionally refuses a
    *first-time* accept outright when the connection's `HostKeyPolicy` is `RequireKnownHost` (that
    policy "only succeeds if the fingerprint is already stored", so it never gets a first-trust UI path);
    it *does* allow `RequireKnownHost` to re-confirm a `Mismatch`, treating that as an explicit
    administrative correction rather than first trust. Exposed as `POST
    /api/v1/connections/{connectionId}/hostKey/probe` (→ `HostKeyProbeDto`) and `POST
    .../hostKey/accept` (body `AcceptSshHostKeyRequestDto{fingerprint}`), mirrored 1:1 as Tauri commands
    `probe_ssh_host_key`/`accept_ssh_host_key`, registered in both `invoke_handler` lists.
  - `fm_connections::ConnectionStatus`/`ConnectionError` and `fm_events::ConnectionStatusPayload`/
    `fm_transport_dto::ConnectionStatusDto`/`ApplicationErrorCode` all gained matching
    `HostKeyUnverified`/`HostKeyMismatch` variants, so `connect_connection`/`test_connection` report a
    status distinct from the generic `failed` bucket (never propagated as an `Err` from `connect`/`test`
    themselves - `ConnectionService::evaluate` already converts every dialer outcome into a tracked
    status, a pattern task 0103 established; this task only added two new status values to that existing
    conversion, it did not change the shape of the conversion itself).
- 2026-08-09: **`fm-ssh`/`fm-vfs-sftp` never depend on `fm-connections`/`fm-credentials`** - a deliberate
  layering decision forced by `fm-test-support`'s architecture fitness test (strictly-downward layers
  only). `fm-connections` sits at layer 2; if `fm-ssh` depended on it directly it would need layer ≥3,
  pushing `fm-vfs-sftp` (which depends on `fm-ssh`) to layer ≥4 - one layer *above* `fm-application`
  (layer 3), making it impossible for `fm-application` to register the provider at all. Resolution:
  `fm-ssh` defines its own connection-agnostic types (`SshConnectTarget`/`SshCredential`/
  `SshHostKeyPolicy`/`SshConnectionParameters`), mirroring how `fm-events::ConnectionStatusPayload`
  already duplicates `fm_connections::ConnectionStatus` for the identical reason. `fm-vfs-sftp` defines
  its own `SshConnectionResolver` trait (given an opaque connection-id string, resolve dial parameters)
  instead of looking up `ConnectionProfile`s itself. `fm-ssh` is layer 1 (zero internal workspace
  deps, like `fm-credentials`); `fm-vfs-sftp` is layer 2 (alongside `fm-vfs-local`). `fm-application`
  (layer 3, the one crate allowed to depend on all four) implements both seams in a new
  `crates/fm-application/src/ssh.rs`: `SshDialer` (`fm_connections::ConnectionDialer` for
  `ConnectionKind::Ssh`, calling `SshConnectionManager::verify_connectivity`) and `SshResolver`
  (`fm_vfs_sftp::SshConnectionResolver`, backed by a second, independent `JsonFileConnectionRepository`
  instance rooted at the same `connections` directory `ConnectionService` itself uses - safe, since the
  repository is a stateless, file-per-connection store with no in-memory cache a second instance could
  desynchronize from). Verified for real: `fm-test-support`'s `workspace_crates_respect_the_documented_layering`
  test (which runs `cargo metadata` against the actual workspace, not a hand-built graph) passes with
  `fm-ssh`/`fm-vfs-sftp` added to `CRATE_LAYERS`.
- 2026-08-09: **Provider implementation** (`SftpFileSystemProvider`, mirrors `fm-vfs-local`'s shape):
  `list`/`metadata`/`inspect`/`file_size`/`create_directory`/`rename`/`remove` (recursive remove
  walks the tree itself via `read_dir`+`remove_file`/`remove_dir`, since SFTPv3 has no native recursive
  delete - mirrors `fm-vfs-local`'s own `remove_dir_all` for the identical reason, not a violation of
  "keep recursive *copy* semantics in the operation engine", which is honoured: the provider never
  walks a directory tree to copy files itself, only `fm-operations`'/`fm-application`'s `CopyExecutor`
  does, via ordinary `list()` calls)/`open_read`/`open_write`/`commit_copy`/`discard_copy`/
  `same_filesystem`/`watch` are all implemented for real; `server_side_copy` is left at its default
  (spec §6.6 "usually limited/unsupported", no portable SFTPv3 primitive exists). `commit_copy`
  publishes a `.fm-copy-{uuid}` file that is itself a *remote* temporary (uploaded by streaming directly
  to it, never staged on local disk) next to the real destination, then `rename`s it into place -
  satisfying spec §6.7 "do not require temporary local files" the same way `fm-vfs-local` does on its
  own filesystem. `capabilities()` reports `LIST | READ | WRITE | CREATE_DIRECTORY | RENAME | MOVE |
  DELETE` only; `RANDOM_ACCESS`/`SET_TIMESTAMPS`/`SET_PERMISSIONS`/`CHECKSUM` are left unset (SFTPv3
  could technically support seek/`fsetstat`, but nothing exercises them against a real server in this
  task, and under-advertising is safer than claiming an unverified capability) - all choices documented
  inline in `provider.rs`'s module doc. `watch` always returns `VfsError::UnsupportedCapability` (no
  default exists on the trait for it; real polling is task 0109's job per the task notes). Transport-level
  errors (`IO`/`Timeout`/`UnexpectedPacket`/`UnexpectedBehavior`) trigger exactly one silent
  reconnect-and-retry per operation (`SftpFileSystemProvider::with_sftp`, backed by
  `SshConnectionManager::invalidate`+`session`); protocol-level responses (`NoSuchFile`,
  `PermissionDenied`, ...) never retry. `same_filesystem` compares the connection-id segment of two
  `sftp://` locations, letting `MoveExecutor` use the provider's own server-native `rename` for a
  same-connection move exactly as it already does for local moves.
- 2026-08-09: **Bug found and fixed during integration testing, not before**: `fm-operations`'s
  `validate_paths` safety preflight (`crates/fm-operations/src/safety.rs`) had a special case comparing
  `archive://` locations by their scheme-stripped text (since they have no native path), but fell
  through to `Location::to_native_path()` - local-filesystem-only - for every *other* provider,
  including the new `sftp` one. A same-connection `SFTP → SFTP` copy therefore failed at the planning
  stage (`SafetyError::IncomparableLocations`) with an opaque "Operation failed" summary and no
  per-item error, purely because both locations shared the `sftp` provider id and hit the local-path
  fallback. Fixed by adding the same kind of scheme-stripped-text special case for `sftp` (keeping the
  connection id as the path's first component, so two different connections are never mistaken for the
  same/nested entry even with textually identical remote paths); regression-tested directly in
  `fm-operations` (`safety_compares_sftp_locations_by_connection_and_path_not_native_path`) and
  indirectly via the real end-to-end same-connection copy/move tests below. This is exactly why the task
  asked for a real `fm-operations`/`fm-application`-level integration test rather than trusting the
  provider-level tests alone - the provider's own isolated tests could not have caught this.
- 2026-08-09: **Frontend**: `frontend/src/features/connections/connections-model.ts` gained
  `isBrowsable(connection)` (true only for `kind === 'ssh'`, the one kind with a real provider - the
  other six are honestly excluded rather than offered as a dead click) and `sftpRootLocation(connectionId)`
  (builds `sftp://<connection-id>/`; the initial path is always `/` - undocumented and un-probeable ahead
  of listing, so it is the one path guaranteed listable regardless of the server's actual home directory,
  and a user can navigate deeper from there like any other pane location), plus the two new status labels.
  `frontend/src/features/panes/pane.ts`'s `SERVERS` group item changed from a static, non-interactive
  `div` into a `button` that calls the same `navigateFavourite(location, attrs)` already used by
  `CLOUD`/`NETWORK`, disabled (with an explanatory `title`) for any non-SSH connection - satisfying "SFTP
  locations open in either pane" while keeping the addition scoped to "open a pane on this connection's
  root" (full context-menu actions are task 0105/spec §5.5, explicitly out of scope here).
  `pnpm run api:export`/`api:generate` regenerated `frontend/openapi/openapi.json` and
  `frontend/src/api/generated/**` (new `HostKeyProbeDto`/`AcceptSshHostKeyRequestDto` models, two new
  `connectionStatusDto`/`applicationErrorCode` enum members, two new client methods); re-running both a
  second time produced no further diff, confirming the checked-in output is stable/in sync.
- 2026-08-09: **Known gaps, documented rather than silent**: (1) SSH agent authentication is not
  implemented (`SshCredential::Agent` reports `SshError::UnsupportedAuthenticationMethod` explicitly,
  never silently ignored) - matches the task's own "start with password/private-key; agent can follow".
  (2) Jump hosts and transfer resume are not implemented - also explicitly named as follow-on work by
  the task. (3) `RANDOM_ACCESS`/`SET_TIMESTAMPS`/`SET_PERMISSIONS`/`CHECKSUM`/`SERVER_SIDE_COPY`/`TRASH`/
  `WATCH` capabilities are all honestly unadvertised (see the provider-implementation note above); no
  caller can be surprised by a claimed-but-unverified capability. (4) No visual "trust this host key?"
  frontend dialog was built - the task's own wording treats plumbing a frontend prompt as optional
  ("if you choose to plumb one through"); the complete, tested backend mechanism (distinct connection
  status, probe/accept REST endpoints mirrored as Tauri commands, generated TypeScript types for both)
  is in place and ready for a future task to build a UI on top of, but today a host-key-pending
  connection surfaces only as a distinguishable status in the connections manager, not an actionable
  prompt. (5) Windows-runtime behaviour of `fm-ssh`/`fm-vfs-sftp` is unverified - both crates are
  ordinary cross-platform crates (not `cfg(target_os = ...)`-gated like `fm-credentials-windows`), and
  SFTP paths are POSIX-style on the wire regardless of client OS by protocol definition, but this was
  only run and tested on macOS; only `cargo check`/`clippy` breadth across the workspace (which does not
  cross-compile) was possible here, matching every other task's identical limitation on this host.
  (6) "No credentials embedded in `Location` URIs" is a structural guarantee (`ParsedSftpUri` has no
  field that could hold one; secrets live only behind a `CredentialStore` reference) rather than a
  dedicated regression test - the same approach task 0103 used for `ConnectionProfile` itself.
- 2026-08-09: Verified (exact commands, not whole-suite totals): `cargo test -p fm-ssh` → 13 passed
  (`src/known_hosts.rs` + `src/fingerprint.rs` unit tests) + `cargo test -p fm-ssh --test
  session_and_host_keys` → 20 passed (password/key auth with/without passphrase, host-key first-use/
  reject/accept/mismatch, probe, connection-manager reuse/reconnect, keepalive, closed-port error
  path); `cargo test -p fm-vfs-sftp --test provider` → 18 passed (capabilities, watch-unsupported,
  mkdir/list/metadata, pagination, upload/download round trip, overwrite refusal, rename, delete
  (file/recursive/non-recursive-non-empty/trash-unsupported), Unicode names, same_filesystem,
  commit_copy/discard_copy, pre-cancelled rejection, dropped-session reconnect); `cargo test -p
  fm-domain --test location_contract` → 12 passed (3 new: `sftp_locations_reference_a_connection_id_rather_than_a_host`,
  `sftp_locations_support_safe_path_navigation`, `sftp_locations_reject_traversal_and_reserved_names`,
  `try_new_validates_the_sftp_provider_matches_the_scheme`); `cargo test -p fm-connections` → 53 passed
  (2 new host-key-status dialer tests); `cargo test -p fm-operations` → 17 passed (1 new safety
  regression test); `cargo test -p fm-application --test ssh_sftp_operations` → 6 passed
  (`local_to_sftp`/`sftp_to_local`/`same_connection_sftp_to_sftp` copies and a same-connection move
  through the real operation engine, mid-transfer cancellation leaving no partial file anywhere, host-key
  probe/accept round trip through the facade, stale-fingerprint rejection); `cargo test -p
  fm-transport-dto` → 71 passed (4 new: `HostKeyProbeDto` tag/camelCase/round-trip, `AcceptSshHostKeyRequestDto`
  round-trip); `cargo test -p fm-server --test connection_routes` → 9 passed (one pre-existing test,
  `connect_then_disconnect_transitions_the_status`, updated: connecting to the fixture's `example.test`
  host now genuinely dials and reports `failed` rather than the pre-0104 "no dialer registered" stand-in
  `connected` - proving the REST layer reaches the real dialer end to end). Full-suite regressions
  checked: `cargo test --workspace` → 743 passed, 0 failed (up from 0103's 718+ baseline, no prior test
  broken by this task's changes beyond the one intentionally-updated assertion above); `cargo clippy
  --workspace --all-targets -- -D warnings` clean; `cargo fmt --all --check` clean.
  `pnpm exec vitest run` (full frontend suite) → 722 passed / 3 failed, the exact same three
  pre-existing failures already documented in 0102's/0103's Agent Notes (theme selector formatting, a
  stale mock action list, the content-search viewer assertion) - confirmed by diffing the failing test
  names against 0103's notes, not just the count; `pnpm exec tsc --noEmit` retains only the same three
  pre-existing errors already documented there (archive creation, a conflict-dialog fixture, the Vite
  configuration); `pnpm exec vitest run src/features/connections/connections-model.test.ts` → 16 passed
  (4 new: distinct host-key-status labels, `isBrowsable` true/false, `sftpRootLocation`); `biome check`
  clean for every file this task touched (generated API files are biome-ignored by design, matching
  `AGENTS.md`'s "never hand-edited" convention).
- 2026-08-09: Follow-up fixes from live user testing. (1) **SSH agent authentication implemented**
  (previously a documented gap): `fm-ssh::session::authenticate_with_agent` connects to the local agent
  via `russh::keys::agent::client::AgentClient::connect_env()` (respects `SSH_AUTH_SOCK`, matching plain
  `ssh`'s own behavior) and tries every plain public-key identity in order via
  `authenticate_publickey_with`, matching OpenSSH's own client behavior; agent-held certificates are
  skipped (a new, smaller documented gap). Verified against a real, hermetic in-process agent server
  (`russh::keys::agent::server::serve` over an ephemeral Unix socket, no external `ssh-agent` process,
  matching `fixture.rs`'s existing hermeticity philosophy): 3 new tests in `fm-ssh/src/session.rs`
  (success with the fixture's authorized key, rejection of a non-matching identity, a typed
  `SshError::Agent` when the agent holds no identities), plus the existing
  `agent_authentication_reports_an_explicit_unsupported_error` integration test rewritten to assert
  against the real environment's agent instead of the old stub. `cargo test -p fm-ssh` → 36 passed (16
  unit + 20 integration). (2) **Dial failure messages were being discarded**: `ConnectionService::evaluate`
  mapped every non-host-key dialer error to a bare `ConnectionStatus::Failed` with no way for a caller to
  see why. Added `ConnectionService::last_error`/`last_error_messages` tracking (set on generic dial
  failure, cleared on any other outcome or explicit disconnect), a new `ConnectionError::DialFailed`
  variant (replacing a misleading reuse of `ConnectionError::Io`), and a `lastError: Option<String>`
  field on `ConnectionDto`, threaded through every REST/Tauri connection endpoint. `cargo test -p
  fm-connections` → 53 passed (existing suite, no behavior regressions); `cargo test -p fm-transport-dto`
  → 72 passed (1 new: `lastError` serializes correctly under `status: "failed"`). (3) Frontend: fixed a
  Mithril "vnodes must either all have keys or none have keys" crash in the `SERVERS` favourites-menu
  group (an unkeyed label sibling to keyed rows), switched every connection-editor text field from
  `onchange` to `oninput` (the `TextInput`/`PasswordInput` controlled-mode contract requires `oninput`;
  `onchange`-only meant any unrelated redraw - frequent here via the SSE stream - silently reverted
  in-progress typing, which is what actually caused "settings not kept"/a name rendering as `Osparkssh`),
  swapped the password/passphrase fields from `TextInput` (which silently ignored an unsupported `type`
  prop) to the real `PasswordInput` component, disabled browser autocapitalize/autocorrect/spellcheck on
  host/username/secret fields, and added a proper `connection-editor.css` (the connections list had no
  styling at all before this) plus an inline `lastError` display so a failed Test/Connect is visible
  without hovering a status dot. Full-suite regressions checked: `cargo test --workspace` → 100% pass,
  `cargo clippy --workspace --all-targets -- -D warnings` clean, `cargo fmt --all --check` clean; `pnpm
  exec vitest run` retains only the same pre-existing baseline failures (confirmed by name, not just
  count); `pnpm exec tsc --noEmit` retains only the same pre-existing baseline errors; verified live
  end-to-end in a real browser against a real backend (typed values survive redraws, password masking,
  no autocapitalize, the `SERVERS` group renders without the fragment-key crash, and a real dial failure
  against an unreachable host now shows a specific inline error message instead of nothing). Known
  remaining gaps, still not silently glossed over: agent-held certificate identities are skipped; no
  frontend UI surfaces `lastError` for the `authenticationRequired`/host-key statuses (only `failed`,
  since those two already have a self-explanatory status label); jump hosts and resumable transfers
  remain out of scope per the original task notes above.
- 2026-08-09: Second follow-up round. (1) Favourites-menu polish per live feedback: the `SERVERS` status
  glyph moved to the right of the name (found and fixed a second instance of the same root cause as the
  earlier `Osparkssh` bug - `.fm-favourites-recents > button`'s `display: block` was silently overriding
  `.fm-server-item`'s `display: flex` at equal specificity, collapsing the flex layout so the glyph ran
  into the text; fixed by matching that selector's specificity exactly); the favourites-popover close
  button now sits in a compact header row (`.fm-favourites-menu-header`, `justify-content: flex-end`)
  instead of reserving a full padded line; `CLOUD`/`NETWORK`/`SERVERS` labels changed to `Cloud`/
  `Network`/`Servers` to match `Favorites`/`Recent locations`' case and size (they already shared the
  same `font-size` rule - the size difference was purely the all-caps text reading larger); `Manage
  connections…` moved to the end of the menu, right-aligned, with its `border-top` separator now
  actually rendering (was defined earlier in the file than a same-specificity rule that reset `border`,
  so cascade order silently discarded it - reordered instead of using `!important`). Also fixed a real,
  **pre-existing** bug unrelated to any of this task's own changes, confirmed still present with every
  change from this and the prior two follow-up rounds stashed: `pane.ts`'s `addCurrentFavourite` called
  `attrs.onAddFavourite(...)` (optional) without a null check, a genuine `tsc` error once surfaced.
  (2) SSH agent authentication reportedly still fails for the user (`the SSH agent has no usable
  public-key identities`) even when run from the exact terminal session where interactive `ssh`/`ssh-add`
  succeed - ruling out the `SSH_AUTH_SOCK`-environment-mismatch theory from the first follow-up round.
  Root cause not yet confirmed. Instrumented `fm-ssh::session::authenticate_with_agent` and its
  `SshCredential::Agent` call site with `tracing::info!`/`warn!` diagnostics (the resolved
  `SSH_AUTH_SOCK` path, every identity the agent reports with algorithm/fingerprint/comment - never key
  material - and the per-identity auth attempt outcome) so the next live test produces an unambiguous
  answer instead of another guess; added `tracing` as an `fm-ssh` dependency for this. (3) While
  re-verifying, found and fixed a genuinely pre-existing, unrelated test flake in
  `apps/fm-desktop/src-tauri/src/event_stream.rs`'s
  `window_teardown_aborts_all_of_its_subscription_tasks_only`: it relied on a single `tokio::task::yield_now()`
  to assume three concurrently spawned subscription tasks had all registered, which only reliably holds
  with little scheduler contention - reproducible under the full workspace suite's parallel test-binary
  load, passed standalone. Replaced with a bounded poll loop matching the established pattern already
  used by the sibling test in the same file; confirmed via two consecutive full `cargo test --workspace`
  runs after the fix, no failures. Full-suite regressions re-checked after all of the above: `cargo test
  --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all --check` all
  clean; `cargo test -p fm-ssh` → 36 passed; frontend `pnpm exec vitest run` → 728 passed / 2 (of the
  same 3 pre-existing baseline) failed - the third simply didn't trigger this run, not a regression;
  `pnpm exec tsc --noEmit` retains only the same 3 pre-existing baseline errors. Verified the favourites-menu
  changes live via Puppeteer against the user's actual running `fm-server` (not a locally-started one -
  confirmed by a bind-conflict when this session tried to start its own), including precise element
  screenshots since a full-window screenshot at that resolution made the bottom-anchored button too small
  to visually confirm by eye alone.
- 2026-08-10: Third follow-up round. (1) **Root-caused the SSH agent mystery**: instrumented logging from
  the prior round proved the agent genuinely reports zero identities (`identity_count=0`) even from the
  user's own terminal session where interactive `ssh`/`ssh-add -l` behave as if a key were loaded - the
  user's `~/.ssh/id_tno` is passphrase-encrypted (confirmed via its `bcrypt`/`aes256-ctr` OpenSSH-v1 header)
  and the system has no `UseKeychain`/`AddKeysToAgent` `ssh_config` defaults, so `ssh` is most likely
  decrypting it directly from disk via a Keychain-cached passphrase on each connection, never actually
  registering it with the agent. No code change from this - it's a genuine environment fact, not a bug -
  but it directly motivated (2). (2) **Added `SecretMaterial`/`ConnectionSecretInputDto::PrivateKeyPath`**
  end to end (`fm-credentials::SecretMaterial` + `codec.rs`, `fm-transport-dto::ConnectionSecretInputDto`,
  `fm-application::connection_dto::secret_material_from_dto`, OpenAPI/Orval regenerated): a private key
  referenced by filesystem path rather than pasted content, read fresh from disk in
  `fm-application::ssh::ssh_connection_parameters` at *dial time* (via `read_private_key_file`, with a
  `~`/`~/`-expansion helper matching shell/`ssh` conventions) rather than stored at rest - deliberately
  mirroring `ssh`'s own `IdentityFile` behavior per the user's explicit ask, and meaning Keychain only ever
  holds a path + passphrase, never key bytes. Reworked `ssh_connection_parameters`'s error type from a bare
  `Result<_, ()>` (which discarded all detail - the dialer previously always mapped failure to
  `ConnectionError::Invalid(vec![])`, an *empty* validation-error list) to `Result<_, String>`, now surfaced
  through the `lastError` mechanism from the second follow-up round via a new `ConnectionError::DialFailed`
  variant on the dialer path and `VfsError::Io{message}` on the VFS-resolver path - both a credential-shape
  mismatch and an unreadable key file now report a real, specific reason instead of silence or an empty
  list. 2 new integration tests in `fm-application/tests/ssh_sftp_operations.rs` cover the full path
  end-to-end against the real fixture SSH server: a real key file on disk successfully authenticating (no
  key bytes ever touch the credential store) and a missing path reporting its exact path in `lastError`.
  Frontend: `connection-editor.ts` gained a `Switch` (`Provide the key as: File path | Pasted content`,
  defaulting to path per the user's stated preference) toggling between the new path input (with a helper
  text explaining the "read fresh, never stored" behavior) and the original paste field; both hosts read
  the path server-side (Tauri: the local machine; browser mode: the fm-server host - the browser itself
  never touches key bytes either way, addressing the user's "even for the browser" question directly).
  (3) Fixed the *actual* root cause of the "select no longer opens" report from live user testing: the
  `Kind` select was rendered with mithril-materialized's `disabled` prop, which blocks the dropdown from
  opening and sets `tabindex="-1"` on `.select-wrapper`, but - confirmed by reading the library's own
  bundled source (`node_modules/mithril-materialized/dist/index.esm.js`) - never sets the underlying
  trigger `<input>`'s HTML `disabled` attribute, so nothing looked disabled while behaving as if it were.
  First attempt replaced it with static text; the user preferred keeping it a real (visually) disabled
  `Select`, so it was reverted and `mithril-materialized-procyon.css` gained rules keyed off the
  `[tabindex="-1"]` signal the library does set, instead of a `:disabled` selector that would never match.
  Also tightened `.fm-connection-form .row` margins (Materialize's un-overridden 20px default stacked
  across 5+ rows in a narrow modal, misread as "misaligned" - the `.row`/`.col` grid math itself was
  already correct). Full-suite regressions re-checked after all of the above: `cargo test --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all --check` all clean; `cargo test
  -p fm-credentials` → 22 passed (2 new), `cargo test -p fm-transport-dto` → 74 passed (2 new), `cargo test
  -p fm-application --test ssh_sftp_operations` → 8 passed (2 new); frontend `pnpm exec vitest run` → 728
  passed / 2 (of the same pre-existing baseline set) failed; `pnpm exec tsc --noEmit` retains only the same
  pre-existing baseline errors; `biome check` clean for every file touched. Verified the disabled-select
  styling, the path/paste toggle (both directions), and the tightened form spacing live via Puppeteer
  against a freshly rebuilt `fm-server`, including full-page and cropped screenshots. Known gap, not
  silently glossed over: the private-key-path feature was verified against the real fixture SSH server's
  own throwaway key, not the user's actual `spark-301b`/`id_tno` - that end-to-end confirmation is still
  the user's to do, since it needs their real passphrase.
