# Procyon

Dual-pane file manager: a Rust workspace (Axum server + Tauri shell) with a Mithril/TypeScript
frontend. See [file-manager-coding-agent-spec.md](file-manager-coding-agent-spec.md) for the full
specification and [TASKS/README.md](TASKS/README.md) for the implementation task index.

[![CI](https://github.com/erikvullings/procyon/actions/workflows/ci.yml/badge.svg)](https://github.com/erikvullings/procyon/actions/workflows/ci.yml)

## Features

**Browsing & navigation**
- Dual-pane layout with a draggable splitter, per-pane tabs, back/forward history, breadcrumb path
  bar and Ctrl/Cmd+L path editing
- Virtualized directory table (only visible rows rendered; verified to 1M entries) with sortable
  Name/Extension/Size/Modified columns, folder grouping and a git-status column
- Grid/icon view with three icon sizes, photo-day grouping, type filtering and a sort menu, plus
  image, video, PDF and CBZ/CBR thumbnails and an F3 fullscreen preview
- Type-to-select quick filter, recursive filename/content search, and durable smart folders with
  structured filters that can open in either pane or a new tab (unsupported provider predicates
  are reported explicitly)
- Favourites/bookmarks and recent locations; cloud-synced folders (iCloud/OneDrive conventions) and
  mounted network volumes surfaced automatically under `CLOUD`/`NETWORK`
- Toggleable directory-tree sidebar (Alt+F10) for hierarchical navigation, with lazy per-node
  expansion, full keyboard support and two-way sync with the active pane's current location, across
  every VFS provider (local, SFTP, FTP, archive, ...)
- WinDirStat-style disk-usage treemap (Ctrl/Cmd+Shift+L) in a separate pane tab, with logical and
  allocated sizes, extension-based colours, and opposite-pane navigation from blocks
- Keyboard-first navigation (arrow/page/edge/range/toggle/select-all/pane-switch) with Total
  Commander-parity shortcuts (see [Keyboard shortcuts](#keyboard-shortcuts) below) and selection
  toggles (invert, select/deselect by glob mask)

**File operations**
- Copy, move, rename, duplicate, create directory, trash and permanent delete — all executed by the
  Rust operation engine (never in TypeScript), with conflict detection (ask/overwrite/rename-new/skip)
- Cancel, pause and resume in flight, with smoothed transfer-rate progress and a queue/history of up
  to 100 completed jobs
- In-app clipboard (copy/cut/paste) and native drag-and-drop, including drag-out to Finder/Explorer
  and drag-in from them
- Multi-rename tool (search/replace, prefix/suffix, sequence numbering, case conversion, live preview)
- Checksums (SHA-256, BLAKE3, CRC32, MD5) and duplicate-file detection
- Directory comparison and synchronization
- Archive browsing and extraction (zip, tar, …), including mutation inside archives and password
  support

**Viewing & editing**
- F3 Lister-style instant large-file viewer with lazy search, virtualized CSV/JSON tables, bounded
  multi-sheet Excel previews, semantic DOCX previews, visually rendered PPTX previews, and native
  playback for short videos; large/MKV videos offer OS-default external playback without loading
  the file into memory
- Excel preview is read-only and uses conservative hard limits: 16 MiB source, 64 MiB expanded
  workbook/string data, 4,096 archive entries, 64 sheets, 100,000 rows and 2,048 columns per sheet,
  500,000 materialized cells, 400,000 non-empty cells, 64 KiB per cell/formula string, 8 MiB per
  image and 16 MiB total images. Files outside these limits remain available externally with the
  limit shown in the viewer. A generated 400,000-cell fixture peaks below 200 MB RSS in the
  development profile on the reference macOS host.
- In-app text editor with Markdown preview (F4)
- File/folder Properties dialog (byte-precise sizes, timestamps, permissions, aggregate totals for
  multi-selection)
- Mounted-volume capacity and directory aggregate totals (size/file count) independent of pagination
- On macOS: Finder tags and Spotlight comments, read/write round-trip compatible with Finder itself

**Remote & cloud**
- SSH/SFTP with pooled, auto-reconnecting sessions and explicit host-key verification (never
  auto-accepted)
- FTP and FTPS (passive FTP, explicit FTPS)
- S3-compatible object storage (AWS S3, MinIO, Cloudflare R2, Backblaze B2, ...) via a configurable
  endpoint URL, with multipart upload for large files
- Embedded terminal drawer (Ctrl+\`/F12) that opens a real remote shell for SSH-backed locations
- Local ↔ SFTP and SFTP ↔ SFTP transfers stream through the same operation engine as local files

**Customization & extensibility**
- Light/dark/follow-system themes; configurable keyboard shortcuts via the action registry
- Command palette (Ctrl/Cmd+P) with fuzzy filtering, ranking and parameter prompts
- Plugins run in a restricted, resource-limited Lua sandbox with per-plugin diagnostics and
  auto-disable on repeated failure; a management UI lists and toggles them
- Sample/bundled plugins: Copy Markdown Path, File Age column, Catppuccin icon theme
- Distributable icon-theme plugins; native file-icon overlay (macOS, Windows)
- Frontend i18n (English + Dutch catalogues) via translate.js

**Platform integration**
- Native app menu bar, Trash/Recycle Bin, Reveal in Finder/Explorer, Open With…, and open-terminal-
  at-location (macOS full support; Windows and Linux support tracked in [ROADMAP.md](ROADMAP.md))
- Selection context menus expose macOS Services and Windows Send To destinations on desktop builds
- Desktop packaging for macOS (`.app`/`.dmg`), Windows (`.msi`/`-setup.exe`) and Linux
  (`.deb`/`.AppImage`) — see [Desktop releases](#desktop-releases)

**Not yet implemented** — external remote-desktop launch and a native SMB provider. Full
status per task lives in [TASKS/README.md](TASKS/README.md); note that
[ROADMAP.md](ROADMAP.md) is a milestone-level summary and can lag behind individual task
completions — each `TASKS/NNNN-*.md` file's own `Status:` header is authoritative.

## Prerequisites

| Tool | Minimum version | Notes |
| --- | --- | --- |
| Rust toolchain | **1.97.1** (stable) | Pinned via `rust-toolchain.toml`. Install via [rustup](https://rustup.rs/). |
| Node.js | **22 LTS** | Managed by [nvs](https://github.com/jasongin/nvs) or nvm. |
| pnpm | **11** | `npm install -g pnpm` or `corepack enable`. |
| cargo-watch | latest | `cargo install cargo-watch` — used in the recommended dev flow. |
| cargo-nextest | latest | `cargo install cargo-nextest --locked` — used by `pnpm run test:rust` (and CI) to run the Rust test suite; doctests still run separately via `cargo test --doc`, since nextest doesn't execute those. |

**Tauri prerequisites (desktop builds only):**

- **macOS**: Xcode Command Line Tools (`xcode-select --install`). No additional WebView runtime
  needed (uses system WebKit).
- **Windows**: Microsoft C++ Build Tools (Visual Studio 2022 or Build Tools for Visual Studio)
  and WebView2 Runtime (ships with Windows 11; installer available at
  <https://developer.microsoft.com/microsoft-edge/webview2/>).
- **Linux**: `webkit2gtk-4.1`, `build-essential`, `curl`, `wget`, `file`, `libssl-dev`,
  `libayatana-appindicator3-dev`, `librsvg2-dev`. See the
  [Tauri prerequisites guide](https://tauri.app/start/prerequisites/) for your distro.

## Repository layout

```
Cargo.toml              workspace root — all Rust crates declared here
package.json            root scripts (dev, test, lint, build, api:*)
apps/
  fm-cli/               thin CLI wrapper (export-openapi subcommand)
  fm-desktop/           Tauri shell (src-tauri/ contains tauri.conf.json)
  fm-server/            Axum HTTP server and SSE endpoint
crates/
  fm-application/       core application service, action registry
  fm-domain/            canonical domain types (entries, locations, …)
  fm-operations/        operation engine: jobs, scheduler, conflict handling
  fm-platform/          platform-adapter trait and capability flags
  fm-platform-macos/    macOS implementation (icons, trash, terminal, native menu bar, …)
  fm-platform-windows/  Windows implementation (Explorer reveal, Recycle Bin, drives, terminal)
  fm-plugin-api/        plugin manifest, permissions, contribution types
  fm-plugin-runtime/    restricted Lua sandbox and plugin lifecycle
  fm-transport-dto/     OpenAPI-serialisable DTOs shared by server and client
  fm-vfs/               VFS provider trait and capability flags
  fm-vfs-local/         local filesystem provider
  fm-vfs-sftp/          SFTP provider (fm-ssh session layer)
  fm-vfs-ftp/           FTP/FTPS provider
  fm-vfs-s3/            S3-compatible object storage provider
  fm-vfs-webdav/        WebDAV (RFC 4918) provider
  fm-connections/       remote connection profiles
  fm-credentials/       credential store abstraction
  fm-credentials-macos/ macOS Keychain backend
  fm-credentials-windows/ Windows Credential Manager backend
  fm-events/            typed event bus and replay buffer
  fm-search/            recursive filesystem search
  fm-settings/          versioned JSON settings with migrations
  fm-ssh/               SSH session and host-key verification
  fm-metadata/          file metadata helpers; pure-Rust thumbnail generation + disk cache
  fm-archive/           archive VFS provider (zip, tar, …)
  fm-checksum/          streaming checksums, checksum files, duplicate detection
  fm-comparison/        directory comparison and sync-plan generation
  fm-vcs-status/        per-directory git working-tree status (git2-backed, cached)
  fm-test-support/      shared test fixtures and helpers
frontend/
  src/                  Mithril/TypeScript sources
  openapi/              checked-in OpenAPI document (do not hand-edit)
docs/
  architecture/         Architecture notes and file-format contracts
  decisions/            Architecture Decision Records (ADRs 0001–0011)
  plugin-api/           Plugin API reference (README.md)
plugins/                Bundled sample plugins (Lua)
TASKS/                  Per-task implementation files (task tracker)
```

## Development

See [AGENTS.md](AGENTS.md) for repository conventions, and run `pnpm run <script>` at the repo
root (`dev`, `test`, `lint`, `build`, ...) — see the root `package.json` for the full list.

### Development commands

| Command | What it does |
| --- | --- |
| `pnpm dev` | Start the Vite dev server with the **mock** client (default). No Rust process needed. |
| `pnpm dev:mock` | Same as `pnpm dev` — mock runtime explicitly selected (`VITE_RUNTIME=mock`). |
| `pnpm dev:http` | Start Vite against the **Axum backend** (`VITE_RUNTIME=http`). Requires Terminal 1 below. |
| `pnpm dev:server` | Start `fm-server` on port 8787 with auth disabled, auto-rebuilding on file change (Terminal 1). |
| `pnpm dev:tauri` | Launch the **Tauri desktop** app in dev mode (`VITE_RUNTIME=tauri`). |
| `pnpm test` | Run Rust tests + frontend tests + script tests. |
| `pnpm test:rust` | `cargo nextest run --workspace` + `cargo test --doc --workspace` (nextest doesn't run doctests) |
| `pnpm test:frontend` | Vitest (frontend unit tests) |
| `pnpm lint` | Rust + Biome linting/formatting checks |
| `pnpm api:export` | Export `frontend/openapi/openapi.json` from the running server |
| `pnpm api:generate` | Regenerate the Orval Fetch client under `frontend/src/api/` |
| `pnpm api:check` | Export + generate and fail if either checked-in file would change |
| `pnpm build` | Production Rust + frontend build |
| `pnpm build:tauri` | Package the Tauri desktop app (`.app`/`.dmg` on macOS, `.msi`/`.exe` on Windows) |

### Recommended two-terminal flow (HTTP mode)

On macOS and Linux:

```bash
# Terminal 1 — Axum backend (auto-rebuilds on file change)
pnpm dev:server

# Terminal 2 — Vite dev server with /api proxy to localhost:8787
pnpm dev:http
```

On Windows, PowerShell has no inline `VAR=value command` syntax, so set the environment variable
as a separate statement (`cargo install cargo-watch` first, if `cargo watch` reports
`no such command`):

```powershell
# Terminal 1 — Axum backend (auto-rebuilds on file change)
$env:FM_SERVER_PORT = '8787'; cargo watch -x 'run -p fm-server -- --dev-mode-auth-disabled'

# Terminal 2 — Vite dev server with /api proxy to localhost:8787
pnpm dev:http
```

The Vite server starts at <http://127.0.0.1:5180>.

**Authentication:** every `/api/v1` route except `/health` and the Swagger/OpenAPI docs requires a
session token by default (task 0064); `fm-server` prints one to stdout at startup. Local dev passes
`--dev-mode-auth-disabled` above because the Vite proxy doesn't attach one — that flag is explicit,
logged at startup, and refused outright if you also try to bind to a non-loopback address. The
frontend probes for this at startup and steps aside automatically when dev mode is active, so local
dev never shows a sign-in prompt; without the flag, it shows one and expects the token printed
above. See [docs/architecture/security.md](docs/architecture/security.md) for the full model.

**How the proxy works:** Vite forwards every `/api/*` request to `http://127.0.0.1:8787`. For SSE
(`GET /api/v1/events`), the proxy configuration in `frontend/config/api-proxy.ts` disables
compression, removes timeouts, and adds `cache-control: no-cache, no-transform` and
`x-accel-buffering: no` so that events are flushed to the browser without buffering.

### Runtimes

The `VITE_RUNTIME` environment variable selects the client adapter at build time:

| `VITE_RUNTIME` | Adapter | Backend needed |
| --- | --- | --- |
| `mock` (default) | In-process mock — fixtures up to 1 M entries | None |
| `http` | HTTP + SSE against Axum | `fm-server` on port 8787 |
| `tauri` | Tauri IPC commands + Tauri channel events | Embedded in the Tauri shell |

### Frontend translations

Application-owned UI copy lives in `frontend/src/i18n/`. English is the canonical catalogue:
add a shallow semantic key such as `pane.newTab` there first, then add the same key to every other
locale. TypeScript enforces exact catalogue parity and rejects unknown keys passed to `t()`.

`translate.js` supports only one group and one sub-key, named placeholders, and basic numeric
plural forms with an `n` fallback. Do not introduce deep dotted keys or locale-specific plural
rules. Use `t.arr()` whenever a placeholder value is a Mithril vnode.

### Swagger UI

When `fm-server` is running, open <http://127.0.0.1:8787/api/v1/docs> to browse the interactive
OpenAPI documentation. The raw OpenAPI JSON is at <http://127.0.0.1:8787/api/v1/openapi.json>.

### Running fm-server on a remote host

`fm-server` binds to loopback only by default, so it isn't reachable from another machine out of
the box — this is deliberate (spec §22, task 0064). To reach it from another device, e.g. a home
automation server you want to browse from your laptop:

**1. `fm-server` is API-only — it doesn't serve the frontend.** Build the frontend
(`pnpm build:frontend`, output in `frontend/dist/`) and serve those static files from a web server
on the remote host. Put that same web server in front of `fm-server` as a reverse proxy for
`/api/*`, so the browser sees the frontend and API on **one origin** — this avoids needing any CORS
configuration at all, and lets the proxy be the single place that terminates TLS. A minimal
[Caddy](https://caddyserver.com/) config does both:

```caddyfile
files.home.example {
    root * /opt/fm/frontend/dist
    file_server
    reverse_proxy /api/* 127.0.0.1:8787
}
```

Caddy obtains and renews a Let's Encrypt certificate automatically from the domain name; use its
`tls internal` directive instead for a self-signed cert on a LAN-only hostname. nginx works the
same way — see the nginx example in
[docs/architecture/security.md](docs/architecture/security.md#7-tlshttps-task-0064) — just add a
`try_files`/static block for `frontend/dist` alongside the existing `/api` proxy block.

**2. Run `fm-server` itself bound to loopback**, behind that proxy, with the roots you actually want
exposed (never leave `--root` empty on a remote host — an empty list allows access to the entire
filesystem):

```bash
fm-server \
  --bind 127.0.0.1 \
  --root /home/pi/media --root /home/pi/backups \
  --max-mutations-per-second 10
```

Because the proxy handles TLS and same-origin routing, `fm-server` needs no `--cors-origin` and no
`--tls-cert`/`--tls-key` here (those exist for the case where you'd rather have `fm-server`
terminate TLS itself and skip the reverse proxy — see
[docs/architecture/security.md](docs/architecture/security.md#7-tlshttps-task-0064) — but fronting
it is simpler for a home box).

**3. Copy the access token to your laptop.** `fm-server` prints one token to stdout at startup
(never pass `--dev-mode-auth-disabled` here — it's refused outright the moment `--bind` isn't
loopback, but don't rely on that guard as your only line of defense). Copy it over SSH or a
password manager, not chat/email. The first time you open `https://files.home.example` from your
laptop, the frontend shows a "Sign in to fm-server" prompt — paste the token there. It's kept in
that browser tab's `sessionStorage` only (never `localStorage`), attached as an `Authorization:
Bearer` header on REST calls and a `?token=` query parameter on the SSE connection (browser
`EventSource` can't set custom headers). If the server restarts — issuing a new token — the next
request gets `401`, which clears the stored token and re-shows the prompt automatically.

**4. If you'd rather skip the reverse proxy entirely** and hit `fm-server` directly from a
separately-hosted frontend (a different origin), bind it to a real interface and set
`--cors-origin` to that exact origin:

```bash
fm-server --bind 0.0.0.0 --cors-origin https://files.home.example \
  --root /home/pi/media --tls-cert cert.pem --tls-key key.pem
```

This logs a startup warning (non-loopback bind) and is more exposure than the proxied setup above
— prefer option 1 unless you have a specific reason not to.

See [docs/architecture/security.md](docs/architecture/security.md) for the full threat model,
including the server-mode TOML config file (`--config`) as an alternative to CLI flags for a
persistent deployment (e.g. a systemd unit).

### Further reading

- [AGENTS.md](AGENTS.md) — coding-agent rules and repository conventions
- [docs/decisions/](docs/decisions/) — Architecture Decision Records (ADR 0001–0011)
- [docs/plugin-api/README.md](docs/plugin-api/README.md) — Plugin API reference
- [TASKS/README.md](TASKS/README.md) — Implementation task index and milestone status
- [ROADMAP.md](ROADMAP.md) — What is done, mocked, and not yet implemented

## Keyboard shortcuts

Every shortcut whose `KeyChord` carries `ctrl: true` is resolved through the app's *primary
modifier* rather than the literal Control key: **Cmd on macOS, Ctrl on Windows/Linux**
(`hasPrimaryModifier` in [`frontend/src/keybindings/dispatcher.ts`](frontend/src/keybindings/dispatcher.ts)).
This mirrors how virtually every native macOS app remaps Windows' Ctrl shortcuts to Cmd, and it
applies uniformly to every shortcut below — there is no per-shortcut opt-out. The one deliberate
exception is **Ctrl+Tab / Ctrl+Shift+Tab** (tab cycling), which always requires the literal Control
key on every platform, because Cmd+Tab is reserved by macOS for the app switcher and never reaches
the page.

A consequence worth knowing: **on macOS, holding literal Control (without Cmd) is not the same as
holding no modifier.** Until a fix landed alongside this table, the dispatcher's bare-key check
only tested for the *absence of Cmd*, so a literal Ctrl-held keypress on macOS looked identical to
an unmodified one and would silently fall through to whatever plain-key binding shared the same
key — e.g. Ctrl+Backspace matched plain Backspace ("parent directory") instead of doing nothing,
and Ctrl+F3…Ctrl+F7 matched the plain F3–F7 bindings (View/Edit/Copy/Move/New Folder) instead of
the intended sort shortcuts. This is now fixed: a bare-key binding requires that *no* modifier
(neither Ctrl nor Cmd) is held, on every platform. Practically, this means literal Ctrl+`<key>` on
macOS is now a safe no-op for anything below rather than a surprising misfire — use Cmd instead.

Separately, **macOS's Mission Control intercepts literal Control+Arrow (Up/Down/Left/Right)
system-wide** for Spaces navigation, before any application — including this one — ever receives
the keypress. This is an OS-level reservation, not something the app can override. It doesn't
actually matter for the shortcuts below, since their real binding is Cmd+Arrow anyway (per the
primary-modifier rule above), but it explains why literal Ctrl+Arrow does nothing at all on macOS
even if Mission Control's own gesture isn't visibly triggered.

| Action | Windows / Linux | macOS | Notes |
| --- | --- | --- | --- |
| Copy | F5 | F5 | |
| Move | F6 | F6 | |
| Rename | F2 | F2 | Shift+F6 is an alias for the same action. |
| View | F3 | F3 | |
| Edit | F4 | F4 | |
| New Folder | F7 | F7 | |
| Trash / Delete | F8, Delete | F8, Delete | Shift+F8 / Shift+Delete for permanent delete when Trash is available. |
| Context menu | Shift+F10 | Shift+F10 | |
| Open With… | Ctrl+Enter | Cmd+Enter | |
| Select All | Ctrl+A | Cmd+A | |
| Copy / Cut / Paste | Ctrl+C / Ctrl+X / Ctrl+V | Cmd+C / Cmd+X / Cmd+V | |
| Favourites (bookmarks) | Ctrl+D | Cmd+D | |
| Refresh | Ctrl+R | Cmd+R | |
| Command Palette | Ctrl+P | Cmd+P | Unavailable in browser runtime (OS/browser reserves Ctrl+P/Cmd+P for print). |
| Focus Location bar | Ctrl+L | Cmd+L | |
| Quick Filter | Ctrl+F | Cmd+F | |
| Find Files | Alt+F7 | Option+F7 | |
| Directory Tree sidebar | Alt+F10 | Option+F10 | Toggles a lazily-expanding tree of the active pane's provider, kept in sync with its current location in both directions. Also available from the command palette. Tab/Shift+Tab cycle between the panes and the sidebar when it's open; the sidebar has its own close button too. |
| Disk Usage treemap | Ctrl+Shift+L | Cmd+Shift+L | Scans the active local directory into a separate tab. Clicking a folder block opens it in the opposite pane. |
| New Tab | Ctrl+T | Cmd+T | Unavailable in browser runtime (browser reserves it). |
| Close Tab | Ctrl+W | Cmd+W | Unavailable in browser runtime (browser reserves it). |
| Close All Tabs | Ctrl+Shift+W | Cmd+Shift+W | |
| Next / Previous Tab | Ctrl+Tab / Ctrl+Shift+Tab | **Ctrl**+Tab / **Ctrl**+Shift+Tab | Always literal Ctrl, even on macOS — Cmd+Tab is OS-reserved for the app switcher. |
| Reopen Closed Tab | Ctrl+Shift+T | Cmd+Shift+T | |
| Jump to Tab N | Ctrl+1…9 | Cmd+1…9 | |
| Go to Root Directory | Ctrl+Backspace | Cmd+Backspace | Literal Ctrl+Backspace on macOS is now a safe no-op (previously misfired as "parent directory"). |
| Open in New Tab | Ctrl+Up | Cmd+Up | Literal Ctrl+Up is swallowed by macOS Mission Control before it reaches the app. |
| Open in New Tab (other pane) | Ctrl+Shift+Up | Cmd+Shift+Up | Literal Ctrl+Shift+Up on macOS is now a safe no-op (previously misfired as "extend selection up"). |
| Duplicate Directory to Other Pane | Ctrl+Left / Ctrl+Right | Cmd+Left / Cmd+Right | Literal Ctrl+Left/Right is swallowed by macOS Mission Control (default Spaces-switching gesture). |
| Swap Pane Directories | Ctrl+U | Cmd+U | Unavailable in browser runtime (browser reserves Ctrl+U/Cmd+U for view-source). |
| Swap Pane Tab Sets | Ctrl+Shift+U | Cmd+Shift+U | |
| New Connection… | Ctrl+N | Cmd+N | Unavailable in browser runtime (browser reserves it). |
| Reactivate Last Quick Filter | Ctrl+Shift+S | Cmd+Shift+S | |
| Show All Files (clear filter) | Ctrl+F10 | Cmd+F10 | |
| Sort by Name / Extension / Date / Size / Unsorted | Ctrl+F3 / F4 / F5 / F6 / F7 | Cmd+F3 / F4 / F5 / F6 / F7 | Literal Ctrl+F3…F7 on macOS is now a safe no-op (previously misfired as View/Edit/Copy/Move/New Folder). |
| Multi-Rename Tool | Ctrl+M | Cmd+M | Also opens automatically on F2 with 2+ entries selected. Not independently verified against a native window menu; flag if Cmd+M ever minimizes the window instead. |
| Properties | Alt+Enter | Option+Return | Shows byte-precise size, timestamps, permissions and provider-specific metadata for the selection; an aggregate (total size, item count, folder/file breakdown) for a multi-selection. |
| Quit | Alt+F4 | Option+F4 | Desktop only. Not independently verified against the OS's own window-close/minimize handling on either platform; macOS users more conventionally expect Cmd+Q, which is not implemented. |
| Keyboard Shortcuts help | F1 | F1 | |
| Toggle selection, advance cursor | Insert | Insert | |
| Restore previous selection | Numpad `/` | Numpad `/` | |
| Create file here | Shift+F4 | Shift+F4 | |
| Duplicate (copy with rename) | Shift+F5 | Shift+F5 | |
| Embedded terminal | Ctrl+\` or F12 | Ctrl+\` or F12 | Desktop only; always literal Ctrl (not translated). |

See [TASKS/0128](TASKS/0128-total-commander-shortcuts-quick-wins.md) for the full Total Commander
parity audit, including shortcuts intentionally *not* implemented (with rationale) and shortcuts
that need real new functionality, tracked separately in
[TASKS/0129](TASKS/0129-total-commander-shortcuts-major-features.md).

For deterministic frontend development without Axum or Tauri, run `pnpm dev:mock`. The mock
adapter provides nested and special-case directory fixtures, configurable loading/failure states,
scriptable backend events, and lazily generated directories of up to 1,000,000 entries.

The custom Mithril directory table uses fixed-height virtual rows from `--fm-row-height`, so large
and lazy mock directories mount only the visible window plus overscan. It exposes semantic grid
rows and cells, cursor/selection rendering hooks, explicit loading/empty/error states, and a
reproducible million-entry rendering check via `pnpm --dir frontend benchmark:directory-table`.
The presentation-only pane composes that table with a compact single-tab strip, clickable
filesystem breadcrumbs, Ctrl/Cmd+L path editing, inline navigation errors, and entry, selection,
size, and sort status counters.
Name, extension, size, and modified headers sort the loaded page in either direction, using stable
natural name ordering and raw metadata values; large sorts yield cooperatively to keep the UI
responsive. Folder grouping comes from the persisted tab view rather than the table component.
A single-letter git status column sits before Modified, always present, populated only for local
directories inside a git working tree (M/S/U/I for modified/staged/untracked/ignored, blank for
clean or non-git entries); a directory's letter is the highest-priority status among its
descendants. `fm-vcs-status` computes it with one `git2` status walk per repository, caches the
result per working-tree root, and invalidates it on the same filesystem-watch-triggered relists
that already refresh the rest of a listing.
The cursor also drives a cancellable lazy metadata summary, while typed size/date presentation
settings keep table and summary formatting consistent.
Per-pane selection is keyed by stable entry IDs and remains independent of the keyboard cursor.
Arrow, page, edge, range, toggle, select-all, pane-switching, open and parent bindings are handled
through the action-registry keybinding dispatcher, with settings overrides, host-platform modifiers
and type-to-select. Numpad `*`, `+`, and `-` invert selection or select/deselect visible files by a
prompted glob mask; top-row `Shift+8`, `Shift+=`, and `-` provide keypad-free equivalents. Bindings
intentionally use character keys because browser keyboard events do not reliably distinguish the
numpad characters across layouts. While a prefix is
active it appears behind a divider at the right of the pane footer, highlights the first matching
in-word occurrence in every matching name, and constrains keyboard cursor movement to those
matches. Backspace edits the prefix, Escape clears it and the selection, and an unmatched prefix
briefly flashes red but remains editable. Non-root directories prepend a synthetic `..` row that
navigates to the parent without entering the selectable file set.
Ctrl/Cmd+P opens a custom, keyboard-first command palette over the already-loaded action registry.
It fuzzy-filters action titles, ids and categories, ranks matches and recently used commands, shows
shortcuts and availability reasons, prompts for schema-defined parameters, and returns focus to its
previous target when closed. Enabled plugin actions use that same registry, so they automatically
appear in the palette and context menus. Plugins currently run in a restricted Lua sandbox with
resource limits and per-plugin bounded diagnostics; Lua failures create non-blocking warnings and
are auto-disabled after repeated failures. See [`docs/plugin-api/README.md`](docs/plugin-api/README.md).
The bundled File Age sample contributes a host-rendered `sample.fileAge` column, with compact age
display and raw modification-time sorting.
The main window loads its authoritative workspace projection through the shared client and renders
the recursive pane layout with a draggable, minimum-width splitter. Pane clicks and Tab traversal
move visible focus through semantic workspace commands; divider changes are sent as debounced
`UpdateLayout` commands. The event-driven operation centre shows queued, running, paused, completed,
and failed jobs with progress, transfer rate, current entry, lifecycle controls, retained results,
and expandable failure details. Completed and failed jobs remain visible until dismissed.
Application-wide settings are stored as versioned JSON in the platform configuration directory
and are shared by the Axum `GET`/`PUT /api/v1/settings` endpoints and equivalent Tauri commands.
Writes are atomic, older schemas migrate forward, and corrupt files are backed up before defaults
are loaded with a warning. Frontend bootstrap applies the stored theme, font/row dimensions, and
date/size formats; live pane layouts, tabs, and per-tab views remain workspace-owned state.

Development builds include Mithril Inspector. Open the docked inspector with the `M` toggle at the
bottom of the page, or press `Alt+Shift+M` to select a rendered element. Use it to trace elements to
their source components, inspect the component tree, and view component attrs and local state. The
inspector and its editor endpoint are excluded from production builds.

Backend-to-frontend updates use one typed event contract for both browser SSE and Tauri channels.
The frontend event-stream abstraction exposes connection status and listener registration while
ignoring unknown future event types for forward compatibility.
Shared frontend data lives in a readonly, explicit Meiosis-style state tree. Typed actions enqueue
immutable Mergerino patches through one animation-frame batch, while targeted subscriptions let
directory and operation views redraw only when their selected slice changes.
Workspace state is a normalized, directory-free projection; directory sessions and transient
cursor, selection, dialog and drag state live in separate slices. All browser, Tauri and mock
workspace mutations use the same semantic `FileManagerClient` command surface, with stale revisions
reloaded and only safely idempotent commands retried.
The Rust event bus assigns monotonic event IDs, filters each subscription by session and workspace,
retains bounded replay history for reconnects, and reports explicit gaps when a client must
resynchronise.
Browser mode exposes that bus as one multiplexed `GET /api/v1/events` SSE connection. Named events
carry the shared typed envelope and numeric replay ID, while observable named keep-alive events let
the frontend detect stale connections. Reconnects resume through `Last-Event-ID` or the browser-safe
`lastEventId` query parameter; expired IDs produce a `resynchronise` event that refetches affected
pane snapshots. Desktop mode forwards the same serialized envelope bytes over one ordered Tauri
channel; channel setup is `connecting`, an installed channel remains `open` until explicit shutdown,
and Tauri does not expose SSE-style `reconnecting`. Directory deltas and operation progress share
the frontend's animation-frame batching policy with SSE. One-off notifications remain on the same
channel to preserve total event ordering and byte parity. Closing a window or disconnecting the
client cancels its Rust subscription task. Connection state is shown textually in the application
header. The Vite `/api` development proxy forwards the stream without compression or buffering.
Until task 0064 introduces production
sessions, REST and SSE share one explicit loopback-only development session; this is not a
production authentication mechanism.

Filesystem access is isolated behind the `fm-vfs` provider contract. Providers advertise explicit
capabilities, expose cancellable asynchronous operations and streaming reads/writes, and are
resolved from provider-neutral locations through a typed registry.

Alongside the static capability flags, every provider reports `TransferCapabilities` for a
*concrete location*: an opaque `TransferEndpoint` naming the backend it lives on, plus whether that
backend supports server-side copy, server-side move, resumable upload/download and random
read/write. Two `sftp://` (or `ftp://`) locations on different saved connections report different
endpoints, so they are never mistaken for one server. The operation planner - not the UI and not
any individual command - selects the transfer strategy from those two capability sets: a
provider-native server-side copy or rename when both sides share an endpoint and support it,
otherwise a direct source-to-destination stream. Progress stays byte/item based and identical
whichever strategy is chosen, and cancellation reaches both the source and the destination provider
before the partially written destination is discarded.
Cloud-synchronized folders discovered from macOS conventions and Windows OneDrive environment
variables appear in the favourites menu under `CLOUD`; they remain ordinary `local` provider
locations and require no vendor credentials. Mounted network volumes discovered from macOS volume
metadata or Windows mapped drives appear separately under `NETWORK`. They also use the existing
`local` provider, preserve optional protocol/server/share and read-only metadata, and require no
embedded SMB client or vendor credentials.

Application-managed remote connections are kept in `fm-connections` and `fm-credentials`. SSH,
FTP/FTPS, WebDAV, S3 and SMB profiles appear in the favourites menu under `SERVERS`; authorized
OneDrive accounts appear under `CLOUD` beside OS-discovered cloud folders. A connection profile never stores a password,
passphrase or token directly - only an opaque reference into a `CredentialStore`, backed by the
macOS Keychain or Windows Credential Manager (an in-memory store is used on other hosts and in
tests only).

Native OneDrive access uses Microsoft Graph rather than an OS-mounted sync folder. The connection
editor opens Authorization Code + PKCE sign-in in the system browser for an existing personal,
work or school Microsoft account; Procyon never collects the Microsoft password and never embeds a
client secret. Each authorized account has its own `onedrive://<connection-id>/` virtual root under
`CLOUD`, while refresh credentials remain behind `CredentialStore`. Personal and Business drives
share browsing, streaming transfer, resumable upload and delta tracking; tenant consent and
Conditional Access decisions remain authoritative and are surfaced rather than bypassed.

SSH/SFTP is implemented by `fm-ssh` (session/authentication/host-key verification, plus a remote
shell channel for the embedded terminal) and `fm-vfs-sftp` (the `FileSystemProvider`, registered
under the `sftp` scheme). A saved connection's `connect`/`test` now perform a real SSH handshake
through a registered dialer; browsing an `sftp://<connection-id>/path` location pools and
transparently reconnects sessions per connection. The embedded terminal drawer (`Ctrl+\``/`F12`)
reuses that same pooled session for a location backed by an SSH connection - opening a terminal
there starts a real remote shell (`cd <path> && exec $SHELL -l` over an SSH `exec` channel,
client-side quoted) instead of a local one, while `core.openTerminal`'s external-terminal launch
remains local-machine-only. SSH host keys are never auto-accepted, first use or on change:
an unverified or changed key surfaces as a distinct connection status
(`hostKeyUnverified`/`hostKeyMismatch`), and`POST /api/v1/connections/{id}/hostKey/probe`/`accept`
(and the equivalent Tauri commands) let a caller inspect the presented fingerprint and explicitly
persist it before a later connect can succeed - accepted fingerprints are stored in a JSON
known-hosts file beside the connection profiles. Clicking a connected server under `SERVERS` opens
its root in the active pane. Every direction pair among `local`, `SFTP` and `FTP/FTPS` -
including `SFTP → FTP` and `FTP → SFTP` - streams through the same operation engine as local
files, with no temporary local file: the bytes go straight from one server to the other and are
staged only in a temporary the destination provider itself owns, which it then publishes
atomically. Same-connection moves use the server-native rename instead of transferring anything.
FTP/FTPS is implemented by `fm-vfs-ftp` (passive FTP and explicit
FTPS, registered under the `ftp` scheme); plain FTP is labelled insecure in the connection editor.
S3-compatible object storage - AWS S3 and any endpoint speaking the same API (MinIO, Cloudflare R2,
Backblaze B2, DigitalOcean Spaces, ...) - is implemented by `fm-vfs-s3`, registered under the `s3`
scheme; the endpoint URL is configurable rather than hardcoded to `amazonaws.com`, and credentials
are an access key id (kept in the typed connection configuration, alongside the bucket/region/key
prefix) plus a secret access key held only behind the connection's `credential_ref`. A bucket has no
real directories: `ListObjectsV2`'s prefix/delimiter semantics stand in for browsing, and
`create_directory` writes a zero-byte marker object whose key ends in `/` (the convention most S3
clients use) rather than silently no-op'ing. S3 has no native rename, so `rename`/`commit_copy`
perform `CopyObject` followed by `DeleteObject`; `server_side_copy` uses a real `CopyObject` within
one bucket, and `read_range` uses a ranged `GetObject`. Uploads stream through a multipart upload
once they exceed a configurable threshold (64 MiB by default, always under S3's 5 GiB single-`PUT`
limit) so a large transfer never buffers the whole file in memory.

WebDAV (RFC 4918) is implemented by `fm-vfs-webdav`, registered under the `webdav` scheme:
directory listing uses `PROPFIND` (depth 1), file operations dispatch through `MKCOL`/`PUT`/`GET`/
`DELETE`/`MOVE`/`COPY`, both Basic and Digest (RFC 2617/7616, `MD5`/`MD5-sess`) authentication are
supported, TLS certificate validation is real (no accept-invalid option exists anywhere in the
crate), and a `423 Locked` response surfaces as a distinct `locked` error rather than a generic
failure. `server_side_copy`/`server_side_move` report `true` (native `COPY`/`MOVE`); `random_read`
reflects whether the connected server has been observed advertising `Accept-Ranges: bytes`, probed
lazily per connection rather than assumed. Native SMB remains unimplemented; its `connect`/`test`
still validates configuration and credential only, without a live handshake.

Neither SFTP, FTP/FTPS, S3 nor WebDAV has a native change-notification API, so unlike the local
provider's real filesystem watch, `fm-application`'s directory service keeps an open remote
directory fresh by conservatively polling it (every 20s, backing off further on repeated failures)
and diffing the
result - see `docs/architecture/filesystem-watching.md` for the full `ChangeTracking` design. A
backgrounded pane can be marked inactive (`POST /api/v1/directories/activity`) to poll such a
location four times less often.

Mutating filesystem work is represented by typed jobs in `fm-operations`. Its bounded scheduler
runs a planning phase before execution, publishes lifecycle and coalesced progress events through
the shared event bus, calculates a smoothed transfer rate, and cooperatively cancels at safe points
with partial-destination cleanup delegated to each operation implementation. Running work can be
paused without losing its planned totals or held scheduler locks, then resumed at the next item or
streaming chunk boundary. Cancellation is surfaced immediately as `Cancelling`, also interrupts
planning and conflict waits, and finishes as `Cancelled` with an explicit partial-progress summary.
Queued jobs expose their FIFO position in the operation centre. Terminal snapshots are retained in
an atomic JSON history beside settings (up to 100 entries and 30 days); an operation found in
flight after restart is retained as `interrupted` with its last known progress and is never resumed.
Shared preflight
checks reject same/nested destinations, case-only renames on insensitive filesystems, traversal
cycles, and file/directory replacement mismatches. Create-directory jobs now execute through the
provider, validate cross-platform-safe names, and create intermediate directories only when the
semantic request explicitly opts in. F7 opens the Materialized new-folder dialog; completion is
reflected through a directory delta that selects and scrolls the new entry into view. Remaining
mutation kinds land incrementally in tasks 0039–0044. Rename jobs use the provider's metadata
operation without copy/delete fallback, reject occupied destinations, and safely handle case-only
changes on insensitive filesystems. F2 opens an inline table editor with basename selection,
client-side validation, Esc cancellation, and Enter commit; stable entry IDs retain cursor and
selection when the directory delta arrives.
Single-file copy jobs stream through provider readers and writers into a private temporary file,
then publish atomically with collision-safe ask, overwrite, and rename-new behavior. F5 copies one
selected file to the other pane; byte/item totals, cancellation cleanup, timestamps, and supported
permissions are handled by the backend operation engine. The precise metadata contract is recorded
in `docs/architecture/file-copy-metadata.md`.
Ctrl/Cmd+C, Ctrl/Cmd+X and Ctrl/Cmd+V retain an in-application clipboard of provider-neutral
locations across panes and tabs. Paste validates the visible destination before it queues a copy or
move operation; cut rows remain dimmed until that move is accepted. System clipboard integration is
kept behind the platform-adapter capability boundary. The command palette and selection context menu
also copy selected filenames, full paths, or paths relative to the active directory as newline-
separated plain text using the host clipboard.
Selected rows can also be dragged between panes or onto loaded tabs. Directory rows resolve as the
destination themselves, invalid/read-only/subtree targets are rejected before drop, and accepted
drops queue the same conflict-safe copy/move operations as paste (move by default; Option on macOS
or Control elsewhere copies). macOS and Windows desktop builds also exchange file-reference drags
with Finder and Explorer; incoming native drops copy through the same conflict-safe operation
engine. Browser and unsupported desktop builds keep this behavior disabled through
`nativeDragOut`.
The same selection context menu appends a capability-gated native submenu on desktop: AppKit
populates Services from the selected file URLs on macOS, while Windows enumerates the current
user's `shell:sendto` folder. Browser, mock, Linux, empty, and non-local selections omit it.
The shared application service now exposes semantic operation start/list/get/cancel/pause/resume
and conflict-resolution methods through matching Axum REST endpoints and Tauri commands. REST
starts accept `Idempotency-Key` so retries return the original job rather than queueing duplicates;
the generated HTTP client and the Tauri and mock adapters expose the same transport-neutral client
surface.

Local paths are represented as validated, percent-encoded `file:` locations rather than raw path
strings. Conversion preserves POSIX, Windows drive, UNC, long-path and Unicode forms; lexical
normalization is constrained to a configured root. See
[`docs/architecture/locations.md`](docs/architecture/locations.md) for the stable URI syntax.

The local provider lists directories in bounded, cancellable pages and fetches detailed metadata
separately. Listings identify dotfiles, Windows hidden attributes, symbolic links and reparse
points without following links. Finder alias resolution remains a later macOS enhancement and is
reported explicitly as unsupported through the `finderAliases` runtime capability.

On macOS, entries can be tagged (colored Finder tags, read lazily per row and shown as small
dots next to the name) and given a Spotlight comment (Get Info's "Comments:" field), both stored
as the same extended attributes Finder itself reads and writes
(`com.apple.metadata:_kMDItemUserTags`/`kMDItemFinderComment`), so tags and comments round-trip
with Finder in either direction. Editing either is reachable from the selection context menu
("Edit Tags…"/"Edit Comment…") via a standalone minimal dialog for now; a future properties/Get
Info dialog may host the same editors instead. Windows and Linux report both capabilities as
unavailable rather than approximating them with a different underlying convention (e.g. NTFS
alternate data streams).

The application layer owns authoritative per-pane directory snapshots, including monotonic
revisions and cancellation of superseded requests. Thin Axum and Tauri adapters expose the same
list, refresh, navigation and metadata operations; listing options include server-side hidden-file
filtering, folder grouping and sorting.

Each frontend pane now loads its active tab's real directory through that shared client surface.
Directory navigation, parent traversal, backend-resolved per-tab history, retryable in-pane errors,
and continuation-token paging are coordinated outside the view components. Superseded requests are
aborted and responses are correlated by request ID before they may replace the visible snapshot.

## CI

`.github/workflows/ci.yml` runs on every push to `main` and every pull request:

- **rust** (matrix: ubuntu-latest, macos-latest, windows-latest): `cargo fmt --all --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo nextest run --workspace`,
  `cargo test --doc --workspace`. Cargo registry/target caching via `Swatinem/rust-cache`, plus
  compiler-output caching via `mozilla-actions/sccache-action` (`RUSTC_WRAPPER=sccache`,
  GitHub Actions cache backend).
- **frontend** (ubuntu-latest): Biome format check, `tsc --noEmit`, Vitest, production build.
  pnpm store caching via `actions/setup-node`'s built-in pnpm cache.
- **desktop** (macos-latest, windows-latest): packaged Tauri build + smoke test, also sccache-backed.
- **audit** (ubuntu-latest, advisory-only — never blocks the workflow): `cargo audit` and
  `pnpm audit`, reporting findings without failing the run.

Pull-request builds never perform code signing or notarization; that is reserved for protected
release workflows (tracked separately).

### Git hooks

`pre-commit` (via husky) formats/lints only the files staged in that commit (`rustfmt`, `clippy`
on the owning crate, `biome`) — fast, since it never touches the full workspace. `pre-push` runs
the same lint checks workspace-wide (`pnpm run lint`: `cargo fmt --all --check`, `cargo clippy
--workspace --all-targets`, `biome check .`) but deliberately does **not** run the test suite —
CI already runs the full suite on every push, so duplicating it locally only doubled the wait
without adding safety. Run `pnpm test` yourself before pushing if you want that assurance locally
too.

### Speeding up local Rust builds across multiple worktrees

If you work out of several `git worktree` checkouts of this repo (as the `.claude/worktrees/`
convention does), each one builds Rust into its own `target/` directory from scratch by default,
which is slow. [`sccache`](https://github.com/mozilla/sccache) caches compiler invocations by
their inputs, so a second worktree building the same dependency versions hits cache instead of
recompiling — without the lock contention a single shared `CARGO_TARGET_DIR` would cause between
concurrent builds in different worktrees. To opt in locally:

```bash
brew install sccache   # or see the sccache README for other platforms
```

then add to `~/.cargo/config.toml` (user-level, not this repo's checked-in `.cargo/config.toml`,
so it doesn't force every contributor/CI runner to have `sccache` installed):

```toml
[build]
rustc-wrapper = "sccache"

# sccache can't cache incremental-compilation artifacts, so leave incremental builds off -
# otherwise most compiles fall back to a normal (uncached) build and sccache barely helps.
# Must be `build.incremental` here, not `[env] CARGO_INCREMENTAL` - the `[env]` table only
# sets variables for compiled binaries/build scripts, not for Cargo's own behavior.
incremental = false
```

Check `sccache --show-stats` after a couple of builds — you should see cache hits climb and the
`incremental` line under "Non-cacheable reasons" disappear; if `incremental` is still there, the
setting above isn't taking effect.

## Desktop releases

Desktop product identity has one source: `[package.metadata.desktop]` in
`apps/fm-desktop/src-tauri/Cargo.toml`. The desktop crate inherits its version from
`[workspace.package]` in the root `Cargo.toml`; `pnpm build:tauri` resolves both through
`cargo metadata` and supplies them to Tauri. On macOS it produces a `.app` and `.dmg`; on Windows
it produces `.msi` and NSIS `-setup.exe` installers; on Linux it produces `.deb` and `.AppImage`
packages. Do not duplicate the version or product identity in `tauri.conf.json`. The base config
contains only a schema-required bootstrap copy of the identifier; the packaging contract test
requires it to match the Cargo-owned value.

To prepare a release:

1. Update `[workspace.package].version` in `Cargo.toml` and refresh `Cargo.lock` with
   `cargo check -p fm-desktop`.
2. Add the user-facing release notes to the GitHub release/tag description or the commits that
   GitHub's generated release notes will collect. Note platform limitations and manual checks.
3. Run `pnpm lint`, `pnpm test`, and `pnpm build:tauri` on a supported desktop host.
4. Commit the version change, then push an annotated `v<version>` tag (for example `v0.2.0`). The
   workflow rejects tags that do not exactly match the Cargo version.

The tag-only `.github/workflows/release-desktop.yml` workflow uses the protected
`desktop-release` GitHub environment. No Apple or Windows signing certificates are required: the
macOS and Windows artefacts are deliberately published unsigned, and the macOS build is not
notarized. Configure only:

- the `HOMEBREW_TAP_REPOSITORY` environment variable as the `owner/homebrew-tap` repository that
  will hold the cask, and `HOMEBREW_TAP_TOKEN` as a fine-grained token allowed to write to it;
- `CHOCOLATEY_API_KEY` as the API key for the `procyon` package on the Chocolatey Community
  Repository.

Pull-request CI does not reference those secrets. The workflow publishes generated release notes,
unsigned macOS and Windows installers, and Linux packages. It then calculates checksums from those
exact release assets, updates `Casks/procyon.rb` in the configured Homebrew tap, and generates and
pushes the Chocolatey package. The unsigned macOS release is a universal binary for Apple Silicon
and Intel Macs. Installing it through Homebrew does not bypass Gatekeeper: users must explicitly
approve the app in macOS Privacy & Security or remove the quarantine attribute only if they trust
the downloaded release. Windows users should expect a Microsoft Defender SmartScreen warning and
must choose to run the installer only after verifying that it came from the official release.

After the first packages have been published, users can install Procyon with:

```sh
brew tap erikvullings/tap
brew install --cask procyon
```

Homebrew 6.0+ requires explicitly trusting third-party taps before their casks/formulae can run
(taps can execute arbitrary Ruby with the user's privileges). If `brew install` refuses to proceed,
trust the tap first:

```sh
brew trust erikvullings/tap
```

or, from an elevated Windows terminal:

```powershell
choco install procyon
```

New Chocolatey package versions may remain unavailable until Community Repository moderation has
completed.

CI performs an unsigned packaging smoke test on disposable macOS and Windows runners: it copies or
installs an artefact, launches the packaged executable, verifies that it remains running, and then
cleans up. Before promoting a release, also perform this manual smoke on each supported platform:

- macOS: download the `.dmg` on a different Mac, mount it, drag Procyon to Applications, confirm the
  expected Gatekeeper warning for the unsigned macOS app, explicitly approve it in Privacy &
  Security, browse a directory, and quit normally.
- Windows: download both installers on a clean Windows VM, confirm the expected SmartScreen warning
  for the unsigned publisher, install one format only after verifying its source, launch Procyon,
  browse a directory, quit, uninstall, and repeat with the other installer format.
- Linux: install the `.deb` on Ubuntu 22.04 or run the `.AppImage` after marking it executable;
  launch Procyon, browse a directory, quit, and remove the installed package or downloaded image.

Auto-update is not included in the first-release packaging design; releases are downloaded and
installed manually.

## Troubleshooting

### macOS: "Unable to show. Denied permissions" opening a mounted drive

If Procyon can't open an otherwise-working mounted volume (an external/USB drive, for example) and
shows **"Unable to show. Denied permissions"**, this is macOS's TCC privacy protection blocking the
app from removable-volume access — Finder is exempt from this check, which is why the same drive
opens fine there. Confirm with:

```bash
ls -la /Volumes/
```

`Operation not permitted` on a specific volume (rather than a normal `drwx` listing) is the
signature of this block; unrelated to standard Unix file permissions.

**Fix:** grant Procyon Full Disk Access.

1. Open **System Settings → Privacy & Security → Full Disk Access**.
2. Click **+**, select **Procyon.app** (in `/Applications`), and toggle it **on**.
3. **Fully quit and relaunch Procyon** (Cmd+Q, not just close the window) — the grant only takes
   effect after the app restarts.

If it's still blocked, also check for a separate **Removable Volumes** entry in the same Privacy &
Security pane (present on some macOS versions) and grant Procyon access there too. If the drive was
last used on a different Mac or user account, right-click it in Finder → **Get Info** and confirm
**"Ignore ownership on this volume"** is checked.
