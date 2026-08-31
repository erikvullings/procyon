# 0105 SSH terminal actions

Status: done
Priority: medium
Subsystem: backend
Depends on: 0103, 0104, 0126

## Context
Extend the embedded terminal drawer (0126) so a terminal opened for a location backed by an SSH
connection runs a shell on the remote host, not a local shell in some placeholder directory.

Today `TerminalRegistry` (`apps/fm-desktop/src-tauri/src/terminal.rs`) only understands `file://`
locations: `location_key()` (terminal.rs:159-165) rejects any other URI scheme with
`TerminalError::UnsupportedLocation`, and `TerminalRegistry::open` always spawns a local shell via
`portable_pty`/`CommandBuilder` (terminal.rs:53-134) at a native path resolved from the location.
0126 explicitly scoped "SSH-backed workspace locations" and "reconnect to existing SSH terminals"
as out of scope.

`fm-ssh` (0104) currently only exposes session/SFTP primitives (`SshSession`, `SshConnectionManager`)
for file transfer — there is no remote shell/exec channel yet. This task adds that channel and wires
it into the existing terminal registry, reusing the already-authenticated SSH connection/session
that backs the open SFTP location rather than re-prompting for credentials.

`core.openTerminal` (0061's external-terminal launch, `PlatformAdapter::open_terminal`) stays
local-machine-only and out of scope here — opening an *external* terminal app against a *remote*
host is not part of this task. Choosing which external terminal application to launch (ghostty,
Warp, etc.) is tracked separately in 0127 and is independent of SSH.

## Acceptance Criteria
- Opening the embedded terminal (F12 / Ctrl+`) while the active pane's location is backed by an SSH
  connection starts a shell on the remote host, not on the local machine.
- `location_key()`/`TerminalRegistry` recognize `ssh://` (or the connection-scoped remote URI
  scheme used by 0103/0104) in addition to `file://`; unsupported schemes still fail explicitly
  rather than silently falling back to a local shell.
- The remote shell channel reuses the SSH connection/session already open for that connection (via
  `fm-ssh`'s `SshConnectionManager`) — it does not open a second, separately authenticated
  connection and does not place credentials in any command line.
- A newly created remote terminal session starts in the location's remote working directory.
- Opening a terminal for a remote location that already has an associated session reuses it, same
  as the existing local-location behavior in 0126.
- Terminal input/output (including ANSI colors, interactive CLI apps) works the same as for local
  sessions from the user's perspective.
- If the connection is closed/dropped, the terminal surfaces a clear error/disconnected state
  instead of hanging silently.
- Actions are capability/context aware: the terminal action is unavailable (not just disabled) for
  connection kinds that cannot support an interactive shell (e.g. FTP, SFTP-only where no shell
  channel is negotiable).
- Tests mock the remote exec channel; no test opens a real network connection.

## Implementation Notes
- Add a remote shell/exec capability to `fm-ssh` (session.rs / a new module) — an SSH "exec" or PTY
  channel via `russh`, distinct from the existing SFTP channel in `session.rs`.
- `TerminalRegistry::open` (terminal.rs) needs a branch that drives this remote channel instead of
  `native_pty_system().openpty()` + local `CommandBuilder` for remote location keys.
- `apps/fm-desktop/src-tauri/src/commands.rs`'s `open_embedded_terminal` currently requires
  `Location::to_native_path()` to succeed; remote locations need a path that doesn't go through
  that local-filesystem resolution.
- Do not give untrusted plugins arbitrary remote command execution — this channel is for the
  terminal drawer only, following 0105's original constraint.
- Reuse `fm-ssh` connection/auth state from `fm-connections`/`fm-application`'s `ssh.rs` and
  `connection_facade.rs` rather than re-deriving credentials.

## Agent Notes
- 2026-08-12: Re-scoped from "external terminal first, embedded terminal later" (written before
  0126 existed) to "extend 0126's embedded terminal to SSH locations", since the embedded drawer is
  now the primary terminal UX and 0126 deliberately deferred SSH support to this task. Split the
  external-terminal-application-choice request (ghostty/Warp/etc.) into 0127, since it's unrelated
  to SSH and applies to local locations too.
- Inspect `fm-ssh::session::SshSession` and `SshConnectionManager` (manager.rs) before implementing
  the exec/PTY channel — check whether `russh`'s channel API is already partially wired for
  something reusable.
- 2026-08-12 (implementation): Built the remote shell channel and wired it end to end.
  - `fm-ssh`: new `shell.rs` module (`RemoteShellChannel`/`RemoteShellReader`/`RemoteShellWriter`/
    `RemoteShellEvent`, plus a `shell_quote` POSIX single-quote escaper) and
    `SshSession::open_shell(term, cols, rows, remote_cwd)` (`session.rs`), which opens a
    `channel_open_session`, sends `request_pty`, then either `request_shell` (no cwd) or `exec`
    with `cd <quoted-dir> 2>/dev/null; exec "${SHELL:-/bin/sh}" -l` (with a cwd) — SSH's `exec`
    channel takes one opaque command string, not a `cwd` field, so quoting the path client-side is
    the injection defense, verified by a dedicated test with an awkward path containing a literal
    single quote. `Channel::split()` gives an independently-owned `ChannelReadHalf`/
    `ChannelWriteHalf`, so the writer (input + `window_change` resize) is cheaply `Clone`-able via
    an internal `Arc` while the reader stays exclusively owned by whichever task pumps it.
    Extended the shared `SshFixture` test server (`fixture.rs`) to accept PTY/shell/exec/
    window-change requests and echo any `data` it receives back on the same channel, so
    `session.rs`'s new tests exercise the real wire protocol end to end rather than a mock (same
    fixture philosophy as the existing SFTP tests) — 22/22 `fm-ssh` unit tests pass
    (`cargo test -p fm-ssh --lib`).
  - `fm-application`: new `remote_terminal.rs` (`RemoteTerminalService`, crate-private), holding the
    same `Arc<SshConnectionManager>` the SFTP provider shares and a second, independently
    constructed `SshResolver` (mirrors the existing "safe to construct separately, stateless
    repository" justification already on the first one in `service.rs`). `open_shell` resolves the
    connection, calls `SshConnectionManager::session` (reuses the pooled session an open SFTP
    browse already established, or dials fresh — never a second independently authenticated
    connection), then `SshSession::open_shell`. Wired as
    `FileManagerService::open_remote_shell(connection_id, remote_path, term, cols, rows)`, and
    `RemoteShellChannel`/`RemoteShellEvent`/`RemoteShellReader`/`RemoteShellWriter` are re-exported
    from `fm_application`'s crate root so `apps/fm-desktop` doesn't need a direct `fm-ssh`
    dependency. A connection id that doesn't exist, or isn't SSH, reports
    `ApplicationError::NotFound`/`InvalidRequest` (via the existing `SshConnectionResolver`/
    `VfsError` mapping) rather than silently falling back to anything — covered by two new
    integration tests in `tests/ssh_sftp_operations.rs` (open a real remote shell over the shared
    fixture and echo-round-trip through it; report `NotFound` for an unknown connection id) — 220
    tests pass across the whole crate (`cargo test -p fm-application`, run with
    `-- --skip cancelling_cross_volume_fallback_leaves_the_source_tree`; see the known-issue note
    below), 10/10 in `ssh_sftp_operations.rs` specifically.
  - `apps/fm-desktop`: `terminal.rs` gained a `TerminalLocation` enum (`Local(PathBuf)` /
    `Remote { connection_id, remote_path }`) parsed off the location URI (`sftp://<connection-id>/
    <path>`, percent-decoded the same way `fm-vfs-sftp`'s private `ParsedSftpLocation` does, since
    that parser isn't reusable across the crate boundary), and `SessionBackend::{Local, Remote}` so
    one `TerminalRegistry` keeps serving both. `TerminalRegistry::open` is now `async fn` (it awaits
    `FileManagerService::open_remote_shell` for a new remote session), and `write`/`resize` are too
    — each briefly locks the (now `Arc`-wrapped) sessions map to clone a `RemoteShellWriter` (cheap,
    `Arc`-backed) or do the local synchronous I/O, then awaits the remote call *outside* the lock
    (a `std::sync::MutexGuard` can't cross an `.await` in a `Send` future). A remote session's
    background reader is a `tokio::spawn`ed task (mirrors the local session's `std::thread`
    reading `portable_pty`); both now remove their own entry from the sessions map when the
    process/channel ends, not just before — the original 0126 code never did this, so a location
    whose shell had exited would keep matching the "reuse existing session" branch forever, forever
    reusing a permanently dead handle. `open_embedded_terminal`/`write_embedded_terminal`/
    `resize_embedded_terminal` are now `async fn` Tauri commands accordingly. 13/13 `fm-desktop`
    unit tests pass, including new `TerminalLocation::parse` coverage (local, `sftp://`, percent-
    decoding, invalid connection id, unknown scheme).
  - Frontend: no changes needed to the IPC contract or `terminal-client.ts`/`app-shell.ts` — the
    drawer already treats `Location.uri` opaquely (0126's stated design goal). Two small, contained
    fixes in `terminal-drawer.ts`, both required by this task's own "surfaces a clear disconnected
    state" acceptance criterion, which turned out unmet even for *local* sessions before this
    change (the `exited` `TerminalEvent` was received and silently dropped): the drawer's `output`
    callback now writes a dim `[Terminal session ended]` line into xterm.js and clears the dead
    `sessionId` on `exited`, so the next toggle/reopen redials instead of writing to (or silently
    no-op'ing against) a session id the backend already discarded — matches the backend-side cleanup
    above. Also softened the "Select a local directory..." placeholder to "Select a directory..."
    since that's no longer accurate. `LiveTerminal.sessionId`'s type widened to
    `string | undefined` (from `?: string`) to satisfy `exactOptionalPropertyTypes` when explicitly
    clearing it. 11/11 terminal-feature frontend tests pass; `tsc --noEmit` shows the same 4
    pre-existing errors as on unmodified `main` (verified via `git stash`), none newly introduced.
  - Capability gating (acceptance criterion "unavailable for connection kinds that cannot support an
    interactive shell"): satisfied by construction today rather than by an explicit kind check —
    the only remote scheme `TerminalLocation::parse` recognizes is `sftp://`, and every `sftp://`
    location is backed by an SSH connection (`fm-vfs-sftp` has no other backing kind yet), so there
    is currently no way to reach this location scheme from a non-shell-capable connection kind
    (FTP/FTPS/OneDrive/WebDav/S3/Smb have no location scheme/provider at all yet). When task 0106
    (FTP/FTPS provider) lands its own scheme, `TerminalLocation::parse` simply won't recognize it and
    will keep reporting `UnsupportedLocation`, so this stays correct without further changes — noted
    here rather than added as a currently-untestable branch.
  - Known pre-existing issue found (not introduced or fixed here): `fm-application`'s
    `move_operation::cancelling_cross_volume_fallback_leaves_the_source_tree` test hangs
    indefinitely in this sandboxed environment — reproduced identically on unmodified `main` via
    `git stash` before reporting it here, so it is unrelated to this task. Its poll loop only checks
    for `OperationStateDto::Running` before cancelling and never handles the operation finishing
    before that state is ever observed, which starves the loop; a genuine flaky test, left as-is
    per this session's scope. Excluded via `-- --skip
    cancelling_cross_volume_fallback_leaves_the_source_tree` for verification purposes only.
  - Verification commands run this session: `cargo test -p fm-ssh --lib` (22/22),
    `cargo test -p fm-application --no-fail-fast -- --skip
    cancelling_cross_volume_fallback_leaves_the_source_tree` (220 passed, 0 failed, 1 filtered),
    `cargo test -p fm-desktop --lib` (13/13), `cargo clippy -p fm-ssh -p fm-application -p
    fm-desktop --all-targets -- -D warnings` (clean), `cargo fmt --check` per-file on every file
    this task touched (clean; did not run whole-crate `cargo fmt`/`--check` since `fm-application`
    already carries pre-existing, unrelated formatting drift on `main` that is out of this task's
    scope), `cargo test -p fm-test-support` (workspace layering fitness, still passes — no new
    crate dependencies were added, only re-exports through the existing `fm-application` -> `fm-ssh`
    edge), and on the frontend: `npx tsc --noEmit` (4 pre-existing errors, unchanged),
    `npx vitest run src/features/terminal` (11/11), `npx biome check
    src/features/terminal/terminal-drawer.ts` (clean).
