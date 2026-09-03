# Task index

Derived from `file-manager-coding-agent-spec.md`. One file per task; every task cites the spec
sections it implements. Tasks are grouped below by functional area for readability — this
grouping carries no ordering weight. What determines pick order is each task's own `Depends on`
header: **pick the lowest-numbered `open` task whose `Depends on` tasks are all `done`.** Check
`TASKS/NNNN-*.md`'s own `Status:` line for ground truth; the checkboxes here are a convenience
index and can lag a live edit by one pass.

```bash
# Terminal 1
cargo watch -x "run -p fm-server"
# Terminal 2
pnpm run dev:http
```

Historically this index was organized by the spec's implementation sequence (§33, "Step 1"
through "Step 10") and by milestone (§16). That ordering is no longer reflected here — the project
has shipped past the original milestone boundaries and grouping by function is more useful for
finding related work. If you need the original sequencing rationale, see git history for this
file before the 2026-08-18 reorganization.

## Foundations & core architecture

Repository bootstrap, the domain model, transport layer (REST/SSE/Tauri), the VFS provider
contract, the event bus, and the workspace service. Everything else depends on this layer.

- [x] 0001 Cargo workspace skeleton and crate stubs
- [x] 0002 Frontend Vite + Mithril + TypeScript skeleton
- [x] 0003 Root development scripts, formatting and linting
- [x] 0004 CI skeleton
- [x] 0005 Architecture documentation and initial ADRs
- [x] 0074 README, development commands and roadmap
- [x] 0006 Core domain model in fm-domain
- [x] 0007 Transport DTOs and OpenAPI schemas
- [x] 0008 Axum server with runtime capabilities, OpenAPI JSON and Swagger UI
- [x] 0009 Deterministic OpenAPI export command
- [x] 0010 Orval-generated Fetch client and api:check
- [x] 0011 FileManagerClient interface and runtime selection
- [x] 0012 HTTP FileManagerClient adapter
- [x] 0013 Mock FileManagerClient adapter and fixtures
- [x] 0014 Typed backend event model and event-stream abstraction
- [x] 0015 Tauri 2 shell application and Tauri client adapter
- [x] 0016 VFS provider trait, capabilities and errors
- [x] 0017 Location parsing and path normalization
- [x] 0018 Local filesystem provider: listing, paging and metadata
- [x] 0019 Directory service, snapshots and request cancellation
- [x] 0020 Filesystem watching and directory deltas *(needs 0031)*
- [x] 0031 Rust event bus
- [x] 0032 SSE endpoint
- [x] 0033 Frontend SSE stream, reconnection and connection status
- [x] 0034 Tauri channel event delivery and transport parity
- [x] 0078 Workspace domain model refinement (§5.3)
- [x] 0079 Workspace repository, validation and default-workspace lifecycle
- [x] 0080 Workspace semantic commands, revisions and REST/Tauri surface
- [x] 0081 Workspace events over the shared event bus *(needs 0031)*
- [x] 0082 Frontend WorkspaceProjection, state slice and command dispatch

## Browsing & navigation

The dual-pane shell: layout, panes, tabs, sorting, selection, and finding things.

- [x] 0021 Frontend application state model
- [x] 0022 CSS variable themes: light, dark and follow-system
- [x] 0024 Virtualized directory table component
- [x] 0025 Pane component: tab strip, breadcrumb path bar and status bar
- [x] 0026 Two-pane workspace layout and pane focus
- [x] 0027 Directory navigation, parent navigation and history
- [x] 0028 Selection model and keyboard navigation
- [x] 0029 Sorting and file metadata summary
- [x] 0067 Quick filter
- [x] 0068 Recursive filesystem search
- [x] 0162 Smart folders and saved searches *(needs 0030, 0068, 0089)*
- [x] 0166 Native indexed search *(needs 0058, 0068, 0089)*
- [ ] 0169 Saved advanced filter presets *(needs 0030, 0067)*
- [x] 0069 Tabs per pane
- [x] 0070 Favourites, bookmarks and recent locations
- [x] 0089 Content search across files
- [x] 0090 Total Commander-style selection toggles (invert, select/deselect by mask)
- [x] 0144 Volumes in Favourites/Go menu, plus Go menu Servers/Cloud/Network sections
- [x] 0139 Directory tree dialog / sidebar tree view *(split out of 0129)*
- [x] 0156 Slow directory navigation (several seconds per folder change) in alpha 5 *(three bugs:
  O(n^2) directory-listing round trips, per-entry async overhead, and the dominant one — watch
  registration blocking the response — see task notes)*

## File operations

Copy, move, rename, delete, and everything that mutates the filesystem — plus the operation
engine, conflict handling, clipboard, drag-and-drop, comparison and checksums that back it.

- [x] 0035 Operation engine core: jobs, scheduler, progress
- [x] 0036 Operations API and operation centre UI
- [x] 0037 Operation: create directory
- [x] 0038 Operation: rename
- [x] 0039 Operation: copy a single file
- [x] 0040 Operation: copy a directory tree
- [x] 0041 Operation: move files and directories
- [x] 0042 Operation: duplicate
- [x] 0043 Operation: move to Trash / Recycle Bin
- [x] 0044 Operation: permanent delete with confirmation
- [x] 0045 Conflict detection, policies and resolution dialog
- [x] 0046 Operation cancellation, pause and resume
- [x] 0047 Operation queue and history
- [x] 0048 In-application clipboard copy / cut / paste
- [x] 0075 Directory comparison and synchronization
- [x] 0077 Checksums and duplicate-file detection
- [x] 0160 Safe operation undo
- [ ] 0161 Saved synchronization profiles *(needs 0030, 0075)*
- [ ] 0163 Durable transfer recovery *(needs 0035, 0047, 0108)*
- [ ] 0165 File collection basket *(needs 0035, 0048, 0108)*
- [ ] 0168 Create symbolic links and Windows shortcuts *(needs 0035, 0058)*
- [x] 0093 Copy filename and path actions
- [ ] 0062 Drag and drop within the app and with the OS *(in_progress — in-app and native
  drag-in/out implemented; interactive Finder/Explorer manual verification still outstanding)*

## Actions, shortcuts & command palette

The action registry that everything (menus, palette, keybindings, plugin contributions) is built
on, plus Total Commander shortcut-parity work.

- [x] 0049 Backend action registry
- [x] 0050 Configurable keybinding dispatcher
- [x] 0051 Command palette
- [x] 0052 Context menus and context-sensitive action availability
- [ ] 0167 Declarative automation recipes *(needs 0035, 0049, 0051)*
- [x] 0128 Total Commander shortcut parity — quick wins
- [ ] 0129 Total Commander shortcut parity — features requiring new subsystems *(scoping task;
  triage each row into its own task, decline, or merge)*

## Viewing, editing & preview

Looking at and editing file contents without leaving the app.

- [x] 0071 Preview service and initial preview panel *(archive-summary/plugin-preview
  extensibility split out to 0141/0142)*
- [x] 0072 Multi-rename tool
- [x] 0076 Archive provider: browse, mutate and passwords
- [x] 0086 F4 edit-in-external-editor action
- [x] 0087 F3 view action
- [x] 0088 Lister-style instant large-file viewer with lazy search
- [x] 0099 In-app text file editor with Markdown preview *(after 0088)*
- [x] 0100 Read-only streaming structured-data viewer
- [x] 0140 File/folder Properties dialog *(split out of 0129)*
- [x] 0141 Archive summary preview *(split out of 0071)*
- [ ] 0142 Plugin-contributed preview renderers *(split out of 0071)*
- [x] 0149 Saved Multi-Rename presets *(needs 0072; quick win layered on the existing rule engine)*
- [x] 0150 Video playback in the F3 Lister viewer *(needs 0088; native `<video>`, mirrors the
  existing `<audio>` path — see the task for the large-file caveat)*
- [ ] 0158 Safe large structured-file editing *(needs 0100; safety gate, copy-only if approved)*
- [x] 0159 Structured viewer Tauri UX regressions *(needs 0100)*
- [x] 0171 DOCX preview in the F3 viewer *(needs 0088)*
- [x] 0172 Bounded spreadsheet preview *(needs 0100)*
- [x] 0173 PPTX content preview *(needs 0088)*
- [x] 0174 macOS Quick Look action *(needs 0059, 0088)*

## Metadata, icons & views

How entries are represented: icons, thumbnails, grid view, git status, extended attributes, and
disk-usage visualization.

- [x] 0085 Directory entry icons (themeable, with optional native-icon overlay)
- [x] 0091 Native file icon overlay (backend-served, layered over 0085) *(after 0085; needs 0059)*
- [x] 0094 Tabler icon subset for the workspace toolbar
- [x] 0096 Mounted volume capacity
- [x] 0097 Directory aggregate totals (size/file count) independent of pagination
- [x] 0130 Windows native file icon extraction *(split out of 0060; layers onto the 0091 overlay
  pipeline)*
- [x] 0134 Thumbnails for images/video and a grid/icon view mode
- [x] 0135 Git status column/badges
- [x] 0136 Extended attributes, Finder tags and Spotlight comments editor
- [x] 0118 Integrate parallel-disk-usage with WinDirStat Treemap View
- [ ] 0170 Perceptual image duplicate detection *(needs 0077, 0134)*
- [ ] 0145 Surface Finder tags/Spotlight comment editing in the Properties dialog *(split out of
  0136; 0140 landed mid-task, after 0136's own standalone dialogs were already built)*
- [x] 0151 Fix Windows git-status/history: `canonicalize()` vs. `git2` path mismatch *(needs 0135;
  uses `dunce::canonicalize()` to keep filesystem and libgit2 paths comparable on Windows)*

## Plugins & extensibility

The Lua plugin runtime, sample plugins, and icon-theme plugins.

- [x] 0053 Plugin API, manifest, discovery and permissions
- [x] 0054 Plugin runtime with error isolation
- [x] 0055 Sample plugin: Copy Markdown Path
- [x] 0056 Sample plugin: File Age column
- [x] 0057 Plugin management UI
- [x] 0092 Catppuccin icon theme *(after 0085)*
- [x] 0095 Distributable icon theme plugins *(after 0053, 0085, 0092)*

## Remote & cloud connections

Everything that reaches a filesystem that isn't the local disk: SSH/SFTP, FTP/FTPS, OS-mediated
cloud and network locations, and the connection framework underneath them.

- [x] 0101 OS cloud-backed locations
- [ ] 0164 Cloud file availability controls *(needs 0058, 0101, 0134)*
- [x] 0102 Mounted network volumes
- [x] 0103 Remote connection framework
- [x] 0104 SFTP provider
- [x] 0105 SSH terminal actions *(extended the embedded terminal drawer to run on the remote host
  over SSH)*
- [x] 0106 FTP and FTPS provider
- [x] 0109 Remote change tracking *(needs 0104, 0106)*
- [ ] 0107 External remote desktop launch
- [x] 0108 Cross-provider transfer planning *(needs 0104, 0106)*
- [x] 0110 Native OneDrive provider *(personal and Microsoft Entra work accounts;
  needs 0103, 0108, 0109)*
- [ ] 0138 OS-level "Mount share…" action *(needs 0102; low priority — only if OS-native mounting
  causes friction)*
- [x] 0146 S3-compatible object storage provider *(needs 0103, 0108, 0109 — unlike 0110/0111, no
  OS mount covers this)*
- [ ] 0147 WebDAV provider *(in_progress — implementation and protocol-fixture coverage complete;
  verification against a real Nextcloud/ownCloud/mod_dav server remains)*

**Parked (freezer)** — not declined outright, just not planned near-term; revisit only if a
concrete need surfaces:

- [ ] 0111 Native SMB provider *(frozen 2026-08-14 — same reasoning: 0102's OS-mediated mounted
  shares already cover the common case; optional, needs 0103, 0108, 0109)*

## Desktop & platform integration

Native OS hooks (Finder/Explorer, Trash, menu bar, terminal), packaging, and desktop-only
behavior.

- [x] 0058 Platform adapter traits and capability reporting
- [ ] 0059 macOS platform integration *(in_progress — core integration is complete; macOS aliases
  still need truthful capability reporting)*
- [ ] 0060 Windows platform integration *(in_progress — core integration is complete; explicit
  `.lnk` opening and real UNC-share verification remain)*
- [x] 0061 Open with default application, reveal in file manager, open terminal
- [ ] 0063 Desktop packaging, signing and notarization *(in_progress — unsigned packaging is
  complete; Apple signing/notarization awaits Developer Program activation, and Windows signing
  remains unconfigured)*
- [x] 0126 Embedded terminal drawer
- [x] 0132 Windows defect: operation routes return 500 / deadlock *(pre-existing, found while
  verifying 0060; blocked the Windows pre-commit hook)*
- [ ] 0133 Populate native menu bar content (macOS + Windows) *(in_progress — macOS manually
  verified; manual Windows visual verification remains)*
- [ ] 0127 External terminal application choice *(pick a specific app, e.g. ghostty/Warp, from the
  context menu)*
- [ ] 0131 Windows native menu bar *(in_progress — HWND hook point and HMENU attachment are
  implemented; manual Windows verification remains)*
- [ ] 0137 Services menu (macOS) / "Send to" (Windows) integration *(in_progress — implementation
  and automated coverage complete; manual macOS and Windows invocation checks remain)*
- [x] 0148 Application deleter (macOS) *(needs 0059, 0061; macOS-only — Windows/Linux already have
  their own uninstall conventions)*

## Settings & workspace management

Persisted app configuration and multi-workspace/window state.

- [x] 0030 Settings service
- [x] 0083 Settings editor UI *(after 0050 and 0057)*
- [x] 0084 Workspace management UI *(after 0069; 0082 already complete)*
- [x] 0143 Workspace last-active restore and per-window desktop placement *(wired up unused
  `WorkspaceService::start`; multi-window support; per-workspace window-frame restore via
  tauri-plugin-window-state; macOS Space placement explicitly out of scope, no public API)*
- [ ] 0157 Workspace/folders not restored on relaunch, and TCC access re-prompts (alpha 5)
  *(in_progress — fixed the "Dock icon does nothing with zero windows" bug (missing
  `RunEvent::Reopen` handler) and stabilized the app identifier/binary name; full TCC persistence
  still needs real Apple Developer ID signing, out of scope here — see task notes)*

## Quality, security & accessibility

Cross-cutting non-feature work: hardening, performance, a11y, diagnostics, i18n, and the
dev-only inspector.

- [x] 0023 Development-only mithril-inspector integration
- [x] 0064 Browser/server mode security hardening (§22)
- [x] 0065 Performance fixtures and benchmarks (§28)
- [x] 0073 Diagnostics view and structured logging (§30)
- [x] 0098 Frontend i18n with translate.js
- [ ] 0066 Accessibility review (§29) *(in_progress — automated axe-core phase complete; manual
  keyboard/screen-reader passes still outstanding)*

## Architecture deepening (internal, non-user-facing)

Refactors to increase module depth, testability, and AI-navigability. No behavior change.

**Frontend** — 0112–0117 complete, but `app-shell.ts` has since regrown to ~3,298 lines (from the
~1,816-line post-0112–0117 low) as new features were bolted onto the same closure. 0153 gave it a
typed controller-composition seam (`app/controller-registry.ts`'s `buildControllers`) that
constructs, wires, and tears down every shell-lifetime controller through one declarative spec
instead of 12 hand-wired `let` + `create*Controller(...)` + scattered teardown call sites — a new
controller now needs one registry entry, not new bespoke wiring code. That seam did *not* shrink
`app-shell.ts`'s line/import count (it went up slightly, ~3,264 → ~3,298): each `*ControllerContext`
object literal still has to live in `app-shell.ts` itself, since it closes over the shell's own
`let`-declared local state — moving it elsewhere isn't possible without either duplicating that
state into a shared object first (a materially larger, separate refactor) or renaming ~126
existing controller-usage call sites throughout the file to route through the registry, which was
judged too large/risky a mechanical sweep for this pass. See 0153's Agent Notes for the full
reasoning; a future pass could revisit the line-count criterion via one of those two routes. 0154
and 0155 (found in the same 2026-08-25 pass) are additional frontend/backend deepening candidates,
not part of the original AppShell decomposition.

- [x] 0112 Extract Operations Controller from AppShell
- [x] 0113 Extract EventHandler Registry from AppShell
- [x] 0114 Decompose Pane Component *(1,324 lines → sub-modules)*
- [x] 0115 Migrate AppShell Closure State to Meiosis Store *(gradual, slice-by-slice)*
- [x] 0116 Centralize Selections-to-Locations Translation
- [x] 0117 Deepen Connections Model with Full Lifecycle
- [x] 0153 Give AppShell a controller composition seam instead of hand-wiring *(composition seam
  built via `buildControllers`; line/import count did not drop, criterion revised — see note above
  and Agent Notes)*
- [x] 0154 Replace global keydown if/else chain with an ordered dispatch table *(54 named routes
  with a directly testable dispatch seam)*

**Backend** — `FileManagerService` (~5,800 lines originally) was decomposed through 0119–0123.
The production facade is now ~1,336 lines of constructor wiring and capability delegation.
Operations, action invocation, search/comparison, checksums, file editing, connections, plugins,
content streaming, and mapping concerns live in dedicated modules with interface-level tests.

- [x] 0120 Extract Operation Planner module *(needs 0119)*
- [x] 0121 Extract File Editor Service *(needs 0119)*
- [x] 0122 Extract Connection Facade *(needs 0119)*
- [x] 0123 Extract Plugin Manager module *(needs 0119)*
- [ ] 0124 Narrow Location URI parsing in fm-domain *(open — provider-specific URI parsing still
  lives in `fm-domain`)*
- [x] 0125 Make Search Engine VFS-provider agnostic *(independent)*
- [x] 0119 Decompose FileManagerService into capability sub-services
- [x] 0152 Give Scheduler::run_job an atomic interruption-state seam *(fm-operations; found and
  fixed a real conflict-resolution deadlock bug along the way — see task notes)*
- [x] 0155 Add unit-level coverage for S3 provider multipart upload paths *(private upload-client
  seam; bounded transient retries and partial-failure aborts covered without the fixture server)*
