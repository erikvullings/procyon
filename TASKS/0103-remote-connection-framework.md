# 0003 Remote connection framework

Status: done
Priority: high
Subsystem: backend
Depends on: none

## Context
Create reusable connection-profile and secure-credential infrastructure for application-managed SSH/SFTP, FTP/FTPS, remote desktop, and future native cloud/SMB providers. This is independent of 0001/0002.

## Acceptance Criteria
- Typed `ConnectionProfile`, `ConnectionId`, `ConnectionKind`, and protocol configurations exist.
- Profiles never persist passwords, passphrases, OAuth tokens, or similar secrets.
- A `CredentialStore` abstraction exists.
- macOS uses Keychain or equivalent protected storage.
- Windows uses Credential Manager or equivalent protected storage.
- Connection CRUD, test/connect/disconnect semantics, and status are exposed through application services.
- Browser/Axum and Tauri clients can manage connections without seeing stored secrets.
- Frontend has an initial `SERVERS`/Connections management surface.
- A newly created browsable connection opens immediately in the active pane.
- Connection-backed tabs keep a plug icon, use the saved connection name at their configured root,
  and hide opaque connection ids from breadcrumbs.
- Tests verify secret separation and lifecycle.

## Implementation Notes
- Suggested crates: `fm-connections`, `fm-credentials`.
- Use tagged typed configs, not generic maps.
- API secret inputs must be write-only; responses never echo secrets.
- Add `connection.statusChanged` events.
- Do not implement a remote filesystem protocol in this task.

## Agent Notes
- Inspect settings persistence and platform abstractions before selecting credential-store implementations.
- 2026-08-30 GitHub Copilot CLI: Unified remote-location presentation and navigation for SSH/SFTP,
  FTP/FTPS, WebDAV, S3, and OneDrive. New connections now open in the active pane as soon as they
  are browsable; OneDrive waits for successful authorization. Tabs use the saved profile name at
  the configured root and retain a plug icon in subfolders. Breadcrumbs show the provider and remote
  path without the opaque connection id. The SFTP provider breadcrumb opens the server filesystem
  root while initial connection navigation still honors its configured start path; WebDAV and S3
  breadcrumbs retain their configured provider roots.
- 2026-08-09: Implemented end to end with TDD. New crates: `crates/fm-credentials` (platform-neutral
  `CredentialStore` trait, `CredentialRef`, `SecretMaterial` tagged enum with hand-written
  secret-redacting `Debug`, `InMemoryCredentialStore` fallback, and a `codec` module used only by
  platform backends to serialize `SecretMaterial` into an opaque blob for real OS-protected
  storage), `crates/fm-credentials-macos` (`#![cfg(target_os = "macos")]`, Keychain generic
  passwords via `security-framework`, added to `[workspace.dependencies]`), `crates/fm-credentials-windows`
  (`#![cfg(target_os = "windows")]`, Windows Credential Manager via `windows-sys`'s
  `Win32_Security_Credentials`), and `crates/fm-connections` (self-contained domain+service crate
  mirroring `fm-search`/`fm-archive`: `ConnectionId`, `ConnectionKind`, `ConnectionConfiguration`
  tagged enum with a fully modeled `SshConnectionConfiguration` per spec §6.3 and minimal typed
  stubs for Ftp/Ftps/OneDrive/WebDav/S3/Smb, `ConnectionProfile`, `ConnectionStatus`,
  `ConnectionRepository` trait plus `InMemoryConnectionRepository`/`JsonFileConnectionRepository`,
  `ConnectionDialer` extension point, and `ConnectionService` with CRUD + connect/disconnect/test).
  All four crates added to `fm-test-support`'s `CRATE_LAYERS` architecture fitness test (credentials
  at layer 1, connections/credentials-macos/credentials-windows at layer 2).
- 2026-08-09: `fm-events::BackendEventPayload` gained `connection.created`, `connection.updated`,
  `connection.statusChanged` (carrying a new `ConnectionStatusPayload`), and `connection.deleted`,
  each keyed by a plain `Uuid` (not a shared id type) to avoid `fm-events` depending on
  `fm-connections`, mirroring how `search.resultsBatch` already carries a bare `Uuid`. Verified event
  delivery needed no further transport wiring: `EventBus`/SSE/Tauri channel dispatch is already
  generic over `BackendEventPayload`.
- 2026-08-09: `fm-application::FileManagerService` gained a `connections: ConnectionService<JsonFileConnectionRepository>`
  field and `list_connections`/`create_connection`/`get_connection`/`update_connection`/
  `delete_connection`/`connect_connection`/`disconnect_connection`/`test_connection` facade methods.
  Added `FileManagerService::with_platform_adapter_and_credential_store` (the real per-host
  constructor) alongside the existing `with_platform_adapter`, which now defaults to
  `InMemoryCredentialStore` for callers (mainly this crate's own tests) that don't care - mirroring
  the existing `PlatformAdapter` selection precedent exactly. `crates/fm-application/src/connection_dto.rs`
  holds the explicit DTO<->domain conversions (kept out of `fm-transport-dto`, which does not and
  should not depend on `fm-connections`/`fm-credentials`, matching how it already has no dependency
  on `fm-search`/`fm-archive`).
- 2026-08-09: **Connect/test scope decision** (documented per the task's Implementation Notes, since
  no SSH/FTP protocol crate exists yet): `ConnectionService::evaluate` validates the typed
  configuration, resolves the credential (if any) through the injected `CredentialStore`, and - only
  if a `ConnectionDialer` is registered for that `ConnectionKind` (none is, by this task) - delegates
  to it; otherwise it reports `Connected` once configuration+credential are confirmed usable. This
  means "Connected" in the current build means "this connection's saved configuration and credential
  are usable", not "a live session was established" - documented prominently in
  `ConnectionService`'s own module doc comment, its facade methods' doc comments, and the REST/Tauri
  handler doc comments, so task 0104/0106 has an unambiguous extension point (`ConnectionDialer::dial`)
  and no future reader mistakes the stand-in for a real handshake. `test` evaluates without mutating
  or publishing the tracked status; `connect`/`disconnect` do, transitioning through `Connecting` and
  publishing `connection.statusChanged` after every change. Deleting a connection deletes its stored
  credential on a best-effort basis (a credential-store failure never blocks the deletion, which is
  authoritative); replacing a credential on update stores the new one first, then best-effort-deletes
  the old one.
- 2026-08-09: `apps/fm-server/src/routes/connection.rs` adds the 8 REST endpoints from spec §16
  (`GET/POST /api/v1/connections`, `GET/PUT/DELETE /api/v1/connections/{connectionId}`,
  `.../connect`, `.../disconnect`, `.../test`), registered in `lib.rs`'s `api_router()`.
  `apps/fm-desktop/src-tauri/src/commands.rs` adds the mirrored Tauri commands, registered in both
  `invoke_handler` lists in `lib.rs` (the real `run()` and the mock-runtime test builder), preserving
  Axum/Tauri parity. Both hosts gained a `credentials.rs` module (`build_credential_store()`)
  mirroring each host's existing `platform.rs`/`platform` module's `cfg(target_os = ...)` adapter
  selection pattern; unlike the platform adapter (which `fm-server` always leaves as
  `FallbackPlatformAdapter` since browser/server mode has no native access to a remote client's OS),
  `fm-server` *does* select a real per-OS credential store, since credential storage is local to
  wherever the server process itself runs, not to the remote client - this satisfies the task's AC
  literally ("macOS uses Keychain... Windows uses Credential Manager") for both hosts, not only the
  desktop one.
- 2026-08-09: `crates/fm-transport-dto/src/connection.rs` adds `ConnectionDto` (response type,
  structurally unable to carry a secret: only a `hasCredential` presence boolean, no
  `credentialRef`/secret field of any kind), `ConnectionConfigurationDto` tagged union,
  `ConnectionSecretInputDto` (write-only request-only secret input, hand-written redacting `Debug` so
  even an accidental `{:?}` log can't leak it), and `CreateConnectionRequestDto`/`UpdateConnectionRequestDto`.
  Found and fixed a real bug during implementation: `#[serde(rename_all = "camelCase")]` on the
  *enum* only renames variant tags, not each struct-like variant's own fields, so
  `ConnectionSecretInputDto::OAuthToken`'s `access_token`/`refresh_token` fields were serializing as
  snake_case until each variant got its own `#[serde(rename_all = "camelCase")]` too; added a
  regression test (`oauth_token_secret_input_uses_camel_case_field_names`) and regenerated the
  OpenAPI/Orval output after the fix, not before.
- 2026-08-09: Frontend: `frontend/src/models/connection.ts` re-exports the generated
  `*ConnectionDto`/etc. Orval types directly as the frontend model types (no normalization needed,
  matching how `WorkspaceLayout`/`WorkspaceSummary` already alias their Dto counterparts).
  `frontend/src/features/connections/connections-model.ts` holds pure list-merge/validation/status-label
  helpers plus thin one-line wrappers over `FileManagerClient` (per AGENTS.md "Move application logic
  into Mithril components" - the manager component only calls `on*` callbacks, never the client
  directly). `frontend/src/features/connections/connection-editor.ts` is a `ModalPanel`-based
  Total-Commander-style manager (list with a `●`/`○` status dot and Connect/Test/Edit/Delete per row,
  toggling to an inline add/edit form) - SSH is the only kind with real typed fields and a write-only
  secret input (password or private key + passphrase, never pre-filled on edit, cleared from
  in-memory form state immediately after a successful save); the other six kinds get minimal typed
  fields with no secret UI (an honestly-scoped, documented gap - no protocol exists for them yet).
  `frontend/src/features/panes/pane.ts`'s favourites popover gained a `SERVERS` group (same pattern as
  the existing `CLOUD`/`NETWORK` groups) listing connections with their status glyph, plus a "Manage
  connections…" action that opens the manager; connection items themselves are not clickable/navigable
  (no remote filesystem provider exists yet, per this task's explicit exclusion). `app-shell.ts` loads
  the connection list at startup alongside system locations, wires the manager's callbacks to
  `connections-model.ts`, and refetches/removes the affected connection on every `connection.*` event
  (`frontend/src/models/events.ts` gained the four new `BackendEventPayload` variants).
- 2026-08-09: Verified (exact commands, not whole-suite totals): `cargo test -p fm-credentials` → 19
  passed; `cargo test -p fm-connections` → 51 passed; `cargo test -p fm-credentials-macos` → 3 passed,
  including a real round-trip store/resolve/delete against **this machine's actual login Keychain**
  (`store_then_resolve_round_trips_through_the_real_keychain`), with no GUI prompt observed; `cargo
  test -p fm-events connection` → 1 new test passed (plus the existing fixture-list test updated to
  include the 4 new variants); `cargo test -p fm-application --lib connection_dto` → 3 passed; `cargo
  test -p fm-transport-dto connection` → 9 passed; `cargo test -p fm-server --test connection_routes`
  → 9 passed (includes asserting the raw HTTP response body never contains a submitted password
  substring, for both create and update); `pnpm exec vitest run src/features/connections/connections-model.test.ts`
  → 12 passed. Full-suite regressions checked: `cargo test --workspace` all green (including the
  `fm-test-support` architecture fitness test), `cargo clippy --workspace --all-targets -- -D
  warnings` clean, `cargo fmt --all --check` clean; `pnpm exec vitest run` (full frontend suite) 718
  passed / 3 failed, all three pre-existing and already documented in 0102's Agent Notes (theme
  selector formatting, a stale mock action list, and the content-search viewer assertion) - confirmed
  by re-running twice, no new failures; `pnpm exec tsc --noEmit` retains only the same three
  pre-existing errors already documented in 0102's Agent Notes (archive creation, a conflict-dialog
  fixture, the Vite configuration); `biome check` clean for every file this task touched (repo-wide
  `pnpm run lint:frontend` retains pre-existing, unrelated diagnostics in files this task never
  touched). `pnpm run api:export`/`api:generate` regenerated `frontend/openapi/openapi.json` and
  `frontend/src/api/generated/**`; re-running both a second time produced no further diff, confirming
  the checked-in output is stable/in sync (`api:check`'s own `git diff --exit-code` step only "fails"
  pre-commit because these are new uncommitted files, not because of drift).
- 2026-08-09: **Known gaps, documented rather than silent**: (1) Windows runtime behavior is
  unverified - `fm-credentials-windows` compiles to an empty crate on this macOS host (by design,
  matching `fm-platform-windows`), so its `CredWriteW`/`CredReadW`/`CredDeleteW` calls and the 2
  tests exercising its pure logic (target-name formatting, wide-string encoding) could not run here;
  only `cargo check -p fm-credentials-windows` and `cargo clippy -p fm-credentials-windows` were
  possible, both clean. This mirrors 0102's Agent Notes' identical limitation for
  `fm-platform-windows`. (2) Non-SSH connection kinds (Ftp/Ftps/OneDrive/WebDav/S3/Smb) have real
  typed configuration structs and DTOs end to end, but no `ConnectionDialer`, no dedicated
  frontend-side client-level validation, and no secret-input UI - intentionally out of scope until
  task 0106 (FTP/FTPS) and the later native-provider tasks land. (3) Multi-user
  browser/server authorization for "which users may see/use which configured connections" (spec §19)
  is not implemented; this repository has no multi-user authentication system yet (task 0064), so
  every connection is currently visible to any caller of the (single, loopback-only) development
  session, matching every other resource this application already exposes the same way.
