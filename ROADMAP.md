# Roadmap

This file tracks which features are **done**, which are **mocked** (UI exists but backed by
in-process fixtures), and what is **not yet implemented** — including every capability currently
reported as `false` by the platform adapter and every area that is platform-untested.

Grouped by functional area rather than by the original spec milestones (§16) — the project has
shipped well past those boundaries and functional grouping is more useful for finding what's done
and what's next. See [TASKS/README.md](TASKS/README.md) for the task-level index behind every
item here; that file's per-task `Status:` header is the source of truth if this page and a task
file ever disagree. Update this file when a task lands.

---

## Foundations & core architecture ✅ COMPLETE

- Rust workspace and all crate stubs; CI (rustfmt/clippy/nextest, Biome/tsc/Vitest, Tauri packaging
  smoke test, advisory-only `cargo audit`/`pnpm audit`)
- Axum HTTP server with health, runtime-capabilities, directory, settings, workspace, and SSE
  endpoints
- OpenAPI generation (`pnpm api:export`) and Orval Fetch client (`pnpm api:generate`)
- `FileManagerClient` interface with HTTP, Tauri, and mock adapters; runtime selection via
  `VITE_RUNTIME`
- Tauri 2 desktop shell — same frontend served by both hosts
- VFS provider trait/capabilities, location parsing/normalization, local filesystem provider
  (listing, paging, metadata), filesystem watching and directory deltas
- Backend event bus with monotonic IDs, session filtering, bounded replay history, and explicit
  gap reporting; SSE endpoint with reconnect via `Last-Event-ID`/`lastEventId`; Tauri channel
  event delivery (same typed envelope, byte-level parity)
- Workspace domain model, repository/lifecycle, semantic commands and REST/Tauri surface, and
  events over the shared event bus; frontend `WorkspaceProjection` state slice

**Not yet implemented:** none — this layer is complete.

---

## Browsing & navigation ✅ COMPLETE

- Two-pane layout with a draggable splitter and pane-focus traversal; per-pane tabs with
  per-tab history
- Virtualized directory table (only visible rows rendered; tested to 1M entries) with sortable
  Name/Extension/Size/Modified columns and folder grouping
- Directory navigation (open, parent, breadcrumb path-bar, back/forward history)
- Type-to-select quick filter (in-word highlighting, red flash on no match), recursive filename
  search, and content search across files
- Selection model (per-pane, keyed by stable IDs, independent of cursor) with keyboard navigation
  (arrow/page/edge/range/toggle/select-all/pane-switch) and selection toggles (invert,
  select/deselect by glob mask)
- Favourites/bookmarks and recent locations; volumes surfaced in the Favourites/Go menu alongside
  Servers/Cloud/Network sections
- Light/dark/follow-system CSS variable themes; Meiosis-style unidirectional state tree with
  Mergerino patches and animation-frame batching

**Not yet implemented:**

- Directory tree dialog / sidebar tree view (`0139`)

---

## File operations ✅ COMPLETE

- Create directory (F7), rename (F2), copy file (F5), copy directory tree, move, duplicate
- Move to Trash/Recycle Bin (macOS and Windows), permanent delete with confirmation
- Conflict detection and resolution dialog (ask / overwrite / rename-new / skip)
- Operation cancellation, pause, and resume at safe boundaries; smoothed transfer-rate progress
  with partial-progress summary on cancel
- Operation centre (queued/running/paused/completed/failed, expandable failure detail) and a
  terminal history (up to 100 entries / 30 days, JSON beside settings)
- In-application clipboard (Ctrl/Cmd+C, X, V) with cut-row dimming and cross-pane paste; copy
  filename/path actions
- Native drag-and-drop within the app and with macOS Finder / Windows Explorer
- Directory comparison and synchronization
- Checksums (SHA-256, BLAKE3, CRC32, MD5) and duplicate-file detection
- All mutations run through the Rust operation engine; no filesystem mutation in TypeScript;
  `Idempotency-Key` support on REST operation start endpoints

**Mocked / partial:**

- Drag-and-drop (`0062`) — in-app and native drag-in/out are implemented; interactive
  Finder/Explorer manual verification is still outstanding

---

## Actions, shortcuts & command palette ✅ COMPLETE

- Backend action registry driving menus, palette, keybindings and plugin contributions
- Configurable keyboard shortcuts (settings overrides, host-platform modifiers)
- Command palette (Ctrl/Cmd+P) — fuzzy filter, rank, recently used, parameter prompts
- Context menus with context-sensitive action availability
- Total Commander shortcut-parity quick wins

**Not yet implemented:**

- Total Commander shortcut parity, features requiring new subsystems (`0129`) — a scoping task;
  each row still needs triage into its own task, decline, or merge

---

## Viewing, editing & preview (mostly complete)

- Preview service and preview panel
- F3 Lister-style instant large-file viewer with lazy search
- F4 in-app text editor with Markdown preview
- Multi-rename tool (search/replace, prefix/suffix, sequence, case, preview before apply)
- Archive browsing and extraction (zip, tar, …), including mutation inside archives and password
  support
- File/folder Properties dialog (byte-precise sizes, timestamps, permissions, aggregate totals
  for multi-selection)

**Not yet implemented:**

- Streaming CSV and Excel file viewer (`0100`)
- Archive summary preview (`0141`, split out of the preview service)
- Plugin-contributed preview renderers (`0142`, split out of the preview service)

---

## Metadata, icons & views (mostly complete)

- Native file icons (backend-served, themeable, layered over an icon-set); Tabler icon subset for
  the workspace toolbar
- Thumbnails for images, video, PDF and CBZ/CBR; grid/icon view with three icon sizes, photo-day
  grouping, type filtering and a sort menu
- Git status column (M/S/U/I, highest-priority status rolled up per directory)
- On macOS: Finder tags and Spotlight comments, read/write round-trip compatible with Finder
  itself
- Mounted-volume capacity and directory aggregate totals (size/file count) independent of
  pagination

**Not yet implemented:**

- Parallel-disk-usage / WinDirStat-style treemap view (`0118`)
- Surfacing Finder tags/Spotlight comment editing inside the Properties dialog (`0145`,
  split out of the extended-attributes work — the standalone editors from that task remain the
  only way to edit them today)

---

## Plugins & extensibility ✅ COMPLETE

- Plugin discovery, manifests (`plugin.toml`), enable/disable, permissions
- Action contributions (command palette + context menu integration); custom metadata columns
  (host-rendered)
- Restricted Lua sandbox with resource limits, per-plugin diagnostics, and auto-disable after
  repeated failures
- Plugin management UI
- Sample/bundled plugins: Copy Markdown Path, File Age column, Catppuccin icon theme; icon themes
  are distributable as plugins

**Not yet implemented:**

- WebAssembly Component Model runtime (spec §19.4 long-term goal; Lua is the current runtime)
- No public native Rust dynamic-library ABI (by design; spec §35)

---

## Remote & cloud connections (mostly complete)

- Remote connection framework (profiles, credentials, REST surface); a connection profile never
  stores a password/passphrase/token directly, only an opaque `CredentialStore` reference
  (macOS Keychain / Windows Credential Manager; in-memory fallback elsewhere)
- SSH/SFTP (`fm-ssh`, `fm-vfs-sftp`) — pooled, auto-reconnecting sessions, explicit host-key
  verification (never auto-accepted), and an embedded terminal drawer that opens a real remote
  shell for SSH-backed locations
- FTP and FTPS (passive FTP, explicit FTPS; plain FTP labelled insecure in the connection editor)
- Cross-provider transfer planning (`0108`) — the operation planner picks provider-native
  server-side copy/move when both endpoints support it, otherwise streams source → destination
  directly with no local temporary file; local ↔ SFTP, local ↔ FTP, SFTP ↔ SFTP, FTP ↔ FTP,
  SFTP ↔ FTP and FTP ↔ SFTP all go through the same operation engine as local files, with
  provider-neutral progress and cancellation reaching both endpoints
- Remote change tracking (polling-based, since neither SFTP nor FTP has a native
  change-notification API)
- OS-mediated cloud-synchronized folders (macOS/OneDrive conventions) and mounted network volumes,
  surfaced under `CLOUD`/`NETWORK` in the favourites menu

**Not yet implemented:**

- External remote-desktop launch (`0107`)
- OS-level "Mount share…" action (`0138`) — low priority, only if OS-native mounting causes
  friction

**Frozen** — parked by product decision, not planned near-term (OS-mediated locations above
already cover the common case for both):

- Native OneDrive provider (`0110`)
- Native SMB provider (`0111`)

---

## Desktop & platform integration (mostly complete)

- Platform adapter trait and capability reporting (frontends respond to capability flags rather
  than detecting the OS directly)
- macOS: native file icons, Reveal in Finder, Trash, Open With…, open terminal at location, native
  app menu bar, native drag-out, Finder tags/Spotlight comments
- Windows: native file icons, Explorer reveal, Recycle Bin, Open With…/Open With chooser, terminal
  integration (`wt.exe`, falling back to `powershell.exe`), drive listing and capacity — all
  manually verified on Windows 11
- Desktop packaging for macOS (`.app`/`.dmg`), Windows (`.msi`/`-setup.exe`), and Linux
  (`.deb`/`.AppImage`) — see [README.md § Desktop releases](README.md#desktop-releases)
- Embedded terminal drawer (Ctrl+\`/F12)

**Not yet implemented:**

- Windows native menu bar (`0131`, split out of the Windows integration work; hook-point-only,
  mirrors the macOS implementation)
- External terminal application choice (`0127`) — pick a specific app (e.g. ghostty/Warp) from
  the context menu instead of the OS default
- Linux has no dedicated platform adapter yet; it runs on the no-op fallback (see the capability
  table below) but still packages and launches via Tauri

---

## Settings & workspace management ✅ COMPLETE

- Settings service: versioned JSON, atomic writes, forward migrations, corrupt-file backup;
  settings editor UI
- Workspace management UI (rename, delete, switch workspaces)
- Workspace last-active restore and per-window desktop placement (multi-window support,
  per-workspace window-frame restore; macOS Space placement is explicitly out of scope — no
  public API for it)

**Not yet implemented:** none.

---

## Quality, security & accessibility (mostly complete)

- Browser/server mode security hardening (session tokens, loopback-only bind by default, dev-mode
  auth flag refused on non-loopback binds)
- Performance fixtures and benchmarks (directory-table rendering check up to 1M entries)
- Diagnostics view and structured logging
- Frontend i18n via translate.js (English + Dutch catalogues, typed catalogue parity)
- `mithril-inspector` integrated in dev builds only (excluded from production)

**In progress:**

- Accessibility review (`0066`) — automated axe-core phase complete; manual keyboard/screen-reader
  passes still outstanding

---

## Platform capabilities

The table below lists every `PlatformCapabilities` bit defined in `fm-platform/src/capabilities.rs`
and whether each host currently reports it as `true`.

| Capability | macOS | Windows | Linux / other |
|---|---|---|---|
| `FILE_ICONS` (native file icons) | ✅ | ✅ | ❌ not implemented |
| `THUMBNAILS` (native thumbnail previews) | ❌ not implemented | ❌ not implemented | ❌ not implemented |
| `REVEAL_IN_FILE_MANAGER` (Reveal in Finder/Explorer) | ✅ | ✅ | ❌ not implemented |
| `TRASH` (move to Trash/Recycle Bin) | ✅ | ✅ | ❌ not implemented |
| `OPEN_WITH_DEFAULT_APPLICATION` | ✅ | ✅ | ❌ not implemented |
| `OPEN_TERMINAL` (open terminal at location) | ✅ | ✅ | ❌ not implemented |
| `CLIPBOARD_FILE_REFERENCES` (OS clipboard file-path lists) | ❌ not implemented | ❌ not implemented | ❌ not implemented |
| `MOUNTED_VOLUMES` (list mounted volumes/drives) | ✅ | ✅ | ❌ not implemented |
| `VOLUME_CAPACITY` (total/available disk space) | ✅ | ✅ | ❌ not implemented |
| `NATIVE_MENUS` (native app menu bar) | ✅ | ❌ not implemented | ❌ not implemented |
| `NATIVE_DRAG_OUT` (drag entries to other apps) | ✅ | ✅ (Tauri only) | ❌ not implemented |
| `EXTENDED_ATTRIBUTES` (Spotlight "Finder comment") | ✅ | ❌ not implemented | ❌ not implemented |
| `FINDER_TAGS` (colored Finder tags) | ✅ | ❌ not implemented | ❌ not implemented |

> **Windows note:** thumbnails need an `HICON`/`IShellItemImageFactory` bitmap re-encoded as PNG,
> and this workspace has no image encoder dependency for that path, so the bit stays unset rather
> than reporting success and failing at call time. Native menus (`0131`) and OS clipboard file
> references still delegate to the fallback adapter.
>
> **Linux note:** no `fm-platform-linux` crate exists yet; Linux uses `FallbackPlatformAdapter`
> (all capabilities unset) via Tauri packaging (`.deb`/`.AppImage`).

---

## Platform-untested areas (§35)

The following areas have been implemented but not verified on all target platforms. Tests that
could not be run on a given platform are noted explicitly in the relevant task's Agent Notes.

| Area | macOS | Windows | Linux |
|---|---|---|---|
| Full build (`cargo build --workspace --release`) | ✅ | CI only (no manual smoke) | CI only |
| Tauri desktop packaging and launch | ✅ | CI smoke (unsigned) | ✅ (AppImage / .deb) |
| Native file icons | ✅ | ✅ manually verified (Windows 11) | N/A (not implemented) |
| Trash / Recycle Bin | ✅ | ✅ manually verified (Windows 11) | N/A (not implemented) |
| Reveal in file manager | ✅ | ✅ manually verified (Windows 11) | N/A (not implemented) |
| Open terminal at location | ✅ | ✅ manually verified (Windows 11, `wt.exe`/`powershell.exe`) | N/A (not implemented) |
| Drag-out to Finder/Explorer | ✅ | CI only — no manual smoke | N/A (not implemented) |
| Credential store (Keychain/Credential Manager) | ✅ macOS Keychain | Not manually verified | In-memory fallback only |
| SSH host-key verification flow | ✅ | Not manually verified | Not manually verified |
| Windows-specific path normalization (UNC, long paths) | N/A | ✅ manually verified | N/A |
| Cross-platform case-only rename on NTFS | macOS (APFS) ✅ | Not manually verified | N/A |
| `CLIPBOARD_FILE_REFERENCES` | Not implemented | Not implemented | Not implemented |

---

## Architecture deepening (internal, non-user-facing)

Refactors to increase module depth, testability, and AI-navigability — no behavior change, so not
tracked as a feature above. Frontend refactors are all complete (AppShell reduced from 3,351 lines
to ~1,816 lines through extraction of 12 focused modules). Backend: 5 of 6 extraction tasks are
done (Operation Planner, File Editor Service, Connection Facade, Plugin Manager, Location URI
parsing, VFS-agnostic search engine); the coordinating task (decomposing `FileManagerService`
itself) is paused — see [TASKS/0119](TASKS/0119-decompose-filemanagerservice.md) for the detailed
state before resuming it.

---

## Further reading

- [TASKS/README.md](TASKS/README.md) — full task index, grouped the same way as this file
- [docs/decisions/](docs/decisions/) — Architecture Decision Records
- [docs/plugin-api/README.md](docs/plugin-api/README.md) — Plugin API reference
- [AGENTS.md](AGENTS.md) — repository conventions and coding-agent rules
- [README.md](README.md) — development setup, commands, and the user-facing feature list
