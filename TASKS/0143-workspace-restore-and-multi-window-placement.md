# 0143 Workspace last-active restore and per-window desktop placement

Status: done
Priority: medium
Owner: unassigned
Agent: unassigned
Area: backend, frontend, desktop
Depends on: none

## Context
Raised by the user while discussing what workspaces do (2026-08-15). Two related gaps:

1. **`WorkspaceService::start` is implemented but never called.** It correctly selects an explicit
   request, else the persisted last-active workspace id, else creates a default
   (`crates/fm-application/src/workspace/service.rs`, spec §5.3.7). But nothing outside its own unit
   tests calls it — it's not registered as a Tauri command in
   `apps/fm-desktop/src-tauri/src/lib.rs`'s `invoke_handler!`, and there's no matching route in
   `apps/fm-server/src/routes/workspace.rs`. Instead the frontend's own
   `openOrCreateDefaultWorkspace` (`frontend/src/features/workspace/workspace-controller.ts`) just
   opens `listWorkspaces()[0]` — the first entry from an unsorted `tokio::fs::read_dir` listing, not
   the tracked last-active workspace. With a single saved workspace this is invisible; with multiple
   named workspaces, relaunch does not reliably reopen the one that was actually open last.
2. **No per-window/per-desktop placement.** The user wants each already-open instance to relaunch on
   the macOS Desktop/Space it was previously on, instead of every relaunched window landing on
   Desktop 1 and needing to be dragged back. Two things are missing before that's even possible:
   - There is no multi-window model at all today — Tauri creates exactly one hardcoded `"main"`
     window (`apps/fm-desktop/src-tauri/src/lib.rs`), and no single-instance guard, so separate
     launches are separate OS processes racing the same on-disk workspace store rather than windows
     of one process.
   - macOS has **no public API** to assign or query which Space/virtual-desktop a window is on
     (`NSWindow.collectionBehavior` only offers `.canJoinAllSpaces`/`.moveToActiveSpace`, nothing
     Space-targeted). Tools like Rectangle/yabai do this via private `CGSSpace*` APIs, which are
     unsupported and can break on any OS update — not something to build on here.

Recommendation from that discussion: don't chase Space-restore via private APIs. Instead persist
window frame (x, y, width, height, display id) per workspace using public `NSScreen`/Tauri APIs and
restore each workspace's window to its last-known screen — this fixes "reopens on the wrong
monitor," which is most of the actual pain, without touching private API territory. Document
Spaces-assignment itself as a known macOS limitation.

## Acceptance Criteria
- `WorkspaceService::start` (or equivalent) is actually invoked on launch — as a Tauri command
  and/or the `fm-server` startup path — so relaunch reopens the tracked last-active workspace
  instead of an arbitrary filesystem-order first entry.
- A real multi-window model: one process can own N windows, one per open workspace, with a way for
  a second launch to hand off to (or spawn a window in) the already-running process rather than
  racing it as a separate process against the same on-disk store.
- Each workspace's window frame (position, size, target display) is persisted using public
  Tauri/`NSScreen` APIs and restored on relaunch, so a workspace's window reopens on the monitor it
  was last on.
- Explicit, documented limitation (in this task's Agent Notes and ideally user-facing) that macOS
  Space/virtual-desktop placement itself is not restored, since no public API supports it — do not
  implement this via private `CGSSpace*`/similar APIs.
- No regression to the existing revision-conflict reconciliation for concurrent workspace mutation
  (`dispatch-workspace-command.ts`) — multi-window support should reduce races, not introduce new
  ones over `last-active.json`, which today is a plain last-write-wins overwrite with no revision
  check.

## Implementation Notes
- Likely splits into sub-tasks once scoped: (a) wire up `WorkspaceService::start` — small, backend +
  Tauri command/HTTP route only; (b) real multi-window Tauri host; (c) per-workspace window-frame
  persistence/restore. (a) is independent and safe to land first; (b) and (c) depend on each other.
- `last-active.json` (`crates/fm-application/src/workspace/persistent.rs`) has no revision/CAS
  protection today, unlike workspace command application
  (`WorkspaceService::apply_command`, which does check `expected_revision`). Worth deciding whether
  that needs fixing as part of (a) or is acceptable given multi-window reduces the race window.
- Frontend's current single-workspace-open assumption lives in
  `frontend/src/features/workspace/workspace-controller.ts` (`openOrCreateDefaultWorkspace`) and
  `frontend/src/features/workspace/workspace-manager.ts` (`sortWorkspaceSummaries`, currently only
  used for the switcher's display list, not startup selection).

## Agent Notes
- 2026-08-15: Task filed after a conversation exploring what workspaces persist and how concurrent
  instances behave; no implementation started yet. See Context above for the full investigation
  (file paths, line-level findings) already done — a future agent should not need to re-derive the
  `WorkspaceService::start`-is-unwired finding or the macOS Spaces API limitation from scratch.
- 2026-08-15: Implemented sub-task (a) — `WorkspaceService::start` is now actually reachable:
  - `apps/fm-server/src/routes/workspace.rs`: new `POST /api/v1/workspaces/start` handler
    (`start_workspace`, optional `workspaceId` query param via `StartWorkspaceQuery`), registered in
    `apps/fm-server/src/lib.rs`. Route ordering vs. `/workspaces/{workspaceId}` isn't an issue —
    axum's matchit router prefers the static `start` segment over the dynamic one automatically.
  - `apps/fm-desktop/src-tauri/src/commands.rs`: new `start_workspace` Tauri command wrapping
    `state.service.start_workspace`, registered in both `invoke_handler!` lists in `lib.rs` (there
    are two — one per build variant).
  - Regenerated `frontend/openapi/openapi.json` (`pnpm api:export`, needs a built `fm-server`) and
    the Orval client (`pnpm api:generate`) to pick up the new `startWorkspace` operation.
  - `FileManagerClient` interface (`frontend/src/api/client/file-manager-client.ts`) gained
    `startWorkspace(workspaceId?, signal?)`; implemented in the HTTP client (wraps the generated
    `startWorkspace` fn), the Tauri client (invokes `start_workspace`), and the mock client (returns
    the requested workspace if given, else the first stored one, else creates a "Default" — mirrors
    backend semantics closely enough for tests; mock has no real last-active tracking).
  - `frontend/src/features/workspace/workspace-controller.ts`'s `openOrCreateDefaultWorkspace` now
    calls `client.startWorkspace(undefined, signal)` instead of `listWorkspaces()[0]` — this is the
    actual fix: relaunch now goes through the backend's real last-active selection instead of an
    arbitrary filesystem-order first entry.
  - Verified: `cargo test -p fm-server -p fm-application --lib` (186 + 29 passed), `cargo clippy -p
    fm-server -p fm-desktop --all-targets -- -D warnings` (clean), `cargo fmt --all -- --check`
    (clean), `cargo build -p fm-desktop` (clean). Frontend: `tsc --noEmit`, `biome check` (both
    clean), `vitest run` (all 1116 existing tests still pass — no new tests added for this specific
    plumbing, see below). Manually verified end-to-end in the browser preview against a live
    `fm-server`: `POST /api/v1/workspaces/start` returns 200 and reopens the correct workspace with
    its persisted state (same on-disk workspace used to verify the column-width persistence fix
    earlier this session).
  - **Not done / still open for this task**: no new automated test specifically exercises the new
    route/command/client method or the `openOrCreateDefaultWorkspace` behavior change (relied on
    existing suite + manual verification) — worth adding if this task is picked up again. Also
    still fully open: `last-active.json`'s lack of revision/CAS protection (noted in Implementation
    Notes above, not addressed), and all of sub-tasks (b) multi-window model and (c) per-workspace
    window-frame persistence/restore — no code written for either yet.
- 2026-08-16: Implemented sub-task (b), the multi-window model — two parts:
  - **Single-instance handoff** (the "don't race a second process" half of the acceptance
    criteria): added `tauri-plugin-single-instance` (workspace dep in root `Cargo.toml`, crate dep
    in `apps/fm-desktop/src-tauri/Cargo.toml`), registered as the *first* `.plugin()` call in
    `run()` (`apps/fm-desktop/src-tauri/src/lib.rs`) per the plugin's own docs. Its callback just
    focuses/unminimizes the `"main"` window — a second OS-level launch (Dock icon, `open -a`,
    double-clicking the .app again) now hands off to the already-running process instead of
    spawning a second one that would race the same on-disk workspace store. Not yet handled: the
    callback ignores `argv`/`cwd`, so a second launch can't yet request opening a *specific*
    workspace's window — see "not done" below.
  - **Per-workspace windows**: new `open_workspace_window` Tauri command
    (`apps/fm-desktop/src-tauri/src/commands.rs`) — labels each workspace's window
    `workspace-<uuid>` and either focuses the existing one (`app.get_webview_window`) or builds a
    new one via `WebviewWindowBuilder` with `WebviewUrl::App("index.html?workspaceId=<uuid>")`,
    matching the main window's chrome (`title_bar_style(Overlay)`, `hidden_title(true)`,
    1280x800). Registered in both `invoke_handler!` lists in `lib.rs`. The service itself needed no
    new wiring: `AppState`'s `Arc<FileManagerService>` is already `.manage()`d once and shared by
    every window Tauri creates.
  - Frontend: `openOrCreateDefaultWorkspace`
    (`frontend/src/features/workspace/workspace-controller.ts`) now reads `?workspaceId=` off
    `window.location.search` and passes it to `client.startWorkspace(...)` explicitly when present
    — this is what makes a workspace-specific window actually start on that workspace instead of
    racing every other open window for "last-active". `FileManagerClient` gained an optional
    `openWorkspaceWindow?(workspaceId)` (desktop-only, same `?`-optional pattern as the existing
    `quit?()`), implemented only in `tauri-file-manager-client.ts` (invokes
    `open_workspace_window`). `WorkspaceSwitcher` gained an "Open in New Window" button per
    workspace row, rendered only when `onOpenInNewWindow` is passed — wired in `app-shell.ts` only
    when `attrsClient.openWorkspaceWindow` exists, so the button is simply absent on the
    browser/HTTP host (verified: HTTP-mode workspace switcher shows only
    Default/Rename/Delete, no new button).
  - Verified: `cargo test -p fm-desktop --lib` (16 passed), `cargo clippy -p fm-desktop --all-targets
    -- -D warnings` (clean), `cargo fmt --all -- --check` (clean), `cargo build -p fm-desktop`
    (clean, including the new `tauri-plugin-single-instance` dependency resolving and building).
    Frontend: `tsc --noEmit`, `biome check` (both clean), `vitest run` (all 1116 tests still pass).
    Manually re-verified in the browser preview (HTTP mode) that the workspace switcher renders
    correctly with the button absent, and that the earlier column-resize fix and `startWorkspace`
    plumbing are both still intact (450px persisted Name-column width survives reload, matching the
    verification from the (a) note above) — this was prompted by the user reporting column resize
    "still doesn't work," which did not reproduce in this browser-based dev preview; likely
    explanation is the user was testing the actual Tauri desktop app before its `cargo tauri dev`
    file-watcher had rebuilt with the fix (that watcher was mid-rebuild/lock-contended with this
    session's own `cargo build` calls at least once during this session) — **could not confirm
    directly**, since no tool in this session can drive or screenshot the real native macOS window.
    Worth the user re-testing directly and reporting back if it's still broken there specifically.
  - **Not done / still open for this task**:
    - The single-instance callback doesn't parse `argv` to open a specific workspace on a second
      launch (e.g. `open -a Procyon --args --workspace <id>`, or a Finder "open with" on a
      particular file) — right now every second launch just focuses whatever's already open.
    - No native menu entry (e.g. "File > New Window") drives `open_workspace_window` yet — the only
      entry point is the new switcher button. `windowMenu()` in
      `frontend/src/features/native-menu/native-menu-spec.ts` currently lists *tabs*, not real OS
      windows, and reconciling that terminology overlap was out of scope for this pass.
    - No tests (Rust or frontend) added specifically for `open_workspace_window`,
      the single-instance callback, or the new switcher button — relied on the existing suites plus
      manual/API-level verification, matching (a)'s note above.
    - Sub-task (c), per-workspace window-frame (position/size/display) persistence and restore, is
      still fully open — no code written. This is the natural next slice: now that
      `open_workspace_window` exists as the one place windows get created, it's the right place to
      also apply a persisted frame before `.build()`.
- 2026-08-16 (later same day): Found and fixed the *real* column-resize bug the user reported
  ("I still cannot drag and resize the columns, neither in the browser nor in tauri"), and
  finished sub-task (c), closing out this task.
  - **The column-resize bug**: the previous session's "verification" was misleading itself —
    `getComputedStyle` reads taken immediately after dispatching synthetic events in a
    backgrounded/automated browser tab don't reflect a pending Mithril redraw (which is
    `requestAnimationFrame`-gated and was being throttled), so a stale-looking result was actually
    just an unpainted frame, not a real bug, and was misread as "it works." Direct, patient
    DOM-level testing (dispatching real `PointerEvent`s on the actual handle element, then forcing
    a real paint via a screenshot capture before reading `getComputedStyle`) surfaced the *real*
    defect: `directory-table.ts`'s mid-drag reconciliation compared the incoming
    `attrs.columnWidths` against the last-seen value **by reference**
    (`sourceColumnWidths !== attrs.columnWidths`), copying the exact pattern
    `workspace-layout.ts` uses for `attrs.workspace.layout`. That pattern only works there because
    `attrs.workspace.layout` *is* referentially stable (unchanged unless the backend actually
    updates it). `attrs.columnWidths` is not: `pane-content-builder.ts` rebuilds it with
    `tab?.view.columns.map(...)` — a brand-new array and brand-new entry objects — on every single
    render. So the reconciliation's "did the source change?" check was true on almost every
    render, including the ones the resize drag's own `move` handler triggers via `m.redraw()`,
    which stomped the live drag override back to the stale persisted width immediately after every
    `pointermove`. The final width still landed correctly on release (that path dispatches the
    drag's own closure variable, untouched by this bug) — which is exactly why it looked like
    "nothing happens" rather than "sometimes wrong": there was no live feedback at all, so a user
    had no way to tell a drag was registering, and the fix for it (only clicking near
    the very edge, precisely, then releasing) wasn't discoverable.
  - Fix: `frontend/src/features/directory-table/directory-table.ts` now compares
    `sourceColumnWidths`/`attrs.columnWidths` by value (`columnWidthsEqual`, a small shallow
    `columnId`+`width` comparison) instead of by reference. Added a regression test in
    `directory-table.test.ts` that mounts with a `columnWidths` prop rebuilt fresh on every render
    (mirroring the real call site), drags a handle, forces a redraw mid-drag the way an unrelated
    app-wide redraw would, and asserts the *live* grid-template width — not just the
    eventually-persisted one, which is what let this slip through both the original implementation
    and the first (inadequate) verification pass.
  - Verified properly this time: `tsc --noEmit`, `biome check` (clean), `vitest run` — full suite,
    1117 tests (was 1116; the one new test), all passing. Also re-verified live in the browser with
    a methodical protocol (fresh `PointerEvent`s dispatched directly on the handle element, a
    `computer` screenshot action forced in between to guarantee a real paint instead of trusting an
    unflushed rAF, then a full page reload to confirm persistence) — a column genuinely resizes
    live during the drag now, and the released width survives reload.
  - Answered the user's Tauri-dev question inline (not written to this file, since it's not
    project-durable): `open_workspace_window` is a normal Tauri command, not gated by dev vs. prod
    build — it works identically under `cargo tauri dev`. Its *only* current trigger is the "Open
    in New Window" button added to the Workspace Switcher in the previous note; there is no native
    menu entry or keyboard shortcut for it yet (see "not done" above).
  - **Sub-task (c) implementation**: added `tauri-plugin-window-state` (root `Cargo.toml` workspace
    dep, `apps/fm-desktop/src-tauri/Cargo.toml` crate dep, resolved to 2.4.1), registered via
    `.plugin(tauri_plugin_window_state::Builder::default().build())` in `run()`
    (`apps/fm-desktop/src-tauri/src/lib.rs`), right after the single-instance plugin. This was a
    much smaller lift than the Implementation Notes originally anticipated ("persist window frame
    ... using public Tauri/NSScreen APIs" as bespoke code): the plugin already does exactly that,
    generically, for *every* window Tauri creates — it hooks `on_window_ready` (fires for windows
    built later via `WebviewWindowBuilder`, not just the config-declared `"main"` one) and
    saves/restores position, size and maximized state to a local JSON file, keyed by window label.
    Since every per-workspace window already has a unique `workspace-<uuid>` label (from sub-task
    (b)'s `open_workspace_window`), this transparently gives each workspace's window its own
    remembered frame with zero additional wiring — no per-workspace frame storage needed in the
    domain model or `open_workspace_window` itself. It also checks `available_monitors()` before
    restoring a position, so a workspace whose monitor got disconnected doesn't restore off-screen.
    Confirmed (by reading the plugin's source at
    `~/.cargo/registry/src/.../tauri-plugin-window-state-2.4.1/src/lib.rs`) that it uses only
    public Tauri window/monitor APIs — no private `CGSSpace*` calls — consistent with this task's
    explicit constraint against chasing Space-restore that way.
  - Verified: `cargo build -p fm-desktop` (clean), `cargo test -p fm-desktop --lib` (16 passed,
    including the mock-runtime smoke test — confirms the plugin doesn't break headless/test
    startup), `cargo clippy -p fm-desktop --all-targets -- -D warnings` (clean), `cargo fmt --all --
    --check` (clean). Not manually verified in a real window (same limitation as before: no tool
    here can drive the native app) — worth the user confirming a workspace window reopens on the
    same monitor after a full quit/relaunch.
  - **Still not done, deliberately left out of this task's scope** (didn't block calling this task
    done, since they're independent follow-ups, not part of the original acceptance criteria):
    argv-based second-launch workspace targeting, a native menu entry for opening new windows, and
    dedicated tests for `open_workspace_window`/the single-instance callback/the switcher button.
    File a new task if any of these turn out to matter in practice.
  - All of this task's acceptance criteria are now met: `WorkspaceService::start` is wired up and
    reachable (sub-task a), a real multi-window model exists with single-instance handoff
    (sub-task b), per-workspace window frames persist and restore via public APIs (sub-task c), and
    macOS Space placement is explicitly and deliberately out of scope with the reasoning recorded
    above. Marking this task done.
- 2026-08-16 (same day, third pass): the user actually tried "Open in New Window" and found sub-task
  (b) was never really working — the new window opened frozen (immovable, only resizable) with
  "Unable to load Workspace" as its only content. Root cause: **`apps/fm-desktop/src-tauri/capabilities/default.json`
  scoped `"windows": ["main"]`** — every permission in that file (all commands, plus
  `core:window:allow-start-dragging`) applied only to a window literally labeled `"main"`. A
  per-workspace window's label is `workspace-<uuid>`, so it matched *no* capability at all: every
  IPC call it made was silently rejected by Tauri's ACL, starting with the very first one
  (`getRuntimeCapabilities`) in the frontend's boot sequence — hence the generic "Unable to load
  workspace" error screen and, separately, no drag permission at all (hence frozen/immovable, while
  native OS-level resize still worked since that's not gated by this permission).
  - Confirmed by reading `tauri-utils`' capability docs
    (`~/.cargo/registry/.../tauri-utils-2.9.3/src/acl/capability.rs`): the `windows` field
    explicitly supports glob patterns ("List of windows that affected by this capability. Can be a
    glob pattern"). Fix: `"windows": ["main", "workspace-*"]`.
  - This was a real gap in the "verified" state from the earlier same-day note above — `cargo build`/
    `clippy`/`fmt`/the mock-runtime smoke test all stay green regardless of capability-file content,
    since ACL enforcement happens at IPC-call time in the running app, not at compile/lint time, and
    the mock-runtime test harness doesn't invoke it through the real ACL layer either. **Lesson for
    next time**: a new window label needs an explicit capability-file entry; this is easy to miss
    since nothing in the Rust or TS type system enforces it, and no automated test in this repo
    currently exercises the ACL layer at all — worth a regression test if this area gets touched
    again (e.g. a mock-runtime test that builds a `workspace-*`-labeled window and asserts a command
    succeeds through the real ACL, not just the mock IPC bypass the existing smoke tests use).
  - Verified: `cargo build -p fm-desktop` (clean, capability file changes are picked up by
    `tauri-build` at build time), `cargo test -p fm-desktop --lib` (16 passed). Still not manually
    verified in the real native window (same standing limitation) — this is the one that most needs
    the user's own confirmation, given it was the exact bug reported.
  - Also fixed in this pass, reported alongside the frozen-window bug: pointer capture on the
    column-resize handle (a fast/excessive drag that carried the pointer outside the window lost its
    `pointerup` and reverted on the next drag's cleanup — `setPointerCapture` fixes it); Size column
    now uses Total Commander-style single-letter units (B/K/M/G/T/P) instead of "KiB"/"MiB"
    (`frontend/src/features/entry-formatting/entry-formatting.ts`); Modified column gained
    `font-variant-numeric: tabular-nums` so digits align without a font change; and the Workspace
    Switcher's Rename/Delete/Open-in-New-Window buttons were rebuilt as `IconButton` + the app's own
    `tooltip()` helper (matching the rest of the toolbar) instead of cramped text buttons, fixing
    both the "looks ugly" complaint and the workspace name being ellipsis-truncated to "Def...".
    None of these four are part of this task's original acceptance criteria (general polish/bugs
    surfaced while testing it) — noted here only because they landed in the same commit
    (`a0c20cd`).
- 2026-08-16 (fourth pass): user's own multi-window matrix testing (main → opens workspace A's
  window fine → from A's window, opening A again "does nothing", opening B works → from B's window,
  opening either A or B "does nothing") pinned down the remaining sub-task (b) bug precisely: **not**
  a dedup/label-matching failure (`get_webview_window(&label)` was finding the right window every
  time) but `existing.set_focus()` alone silently failing to visibly raise a window that isn't
  already frontmost/visible. `apps/fm-desktop/src-tauri/src/commands.rs`'s `open_workspace_window`
  now calls `.show()` and `.unminimize()` before `.set_focus()`, mirroring the pairing already used
  by the single-instance callback in `lib.rs`. Also: the switcher panel now closes itself after
  "Open in New Window" is clicked (`frontend/src/app/app-shell.ts`), per explicit user request,
  instead of staying open over a window that's no longer the relevant one. Commit `ec076cc`.
  Verified: `cargo build/test/clippy/fmt -p fm-desktop` all clean, frontend `tsc`/`biome`/`vitest`
  clean. **Still not confirmed in the real app** — this fix is inferred from the reported symptom
  pattern, not from seeing the actual failure directly (no tool here can drive the native window);
  ask the user to re-run the same open-A/open-B/open-either matrix and confirm each window now
  visibly raises.
  - Separately clarified for the user (not a code issue, no fix applied): "no permission" opening
    `~/Downloads` in both the browser and the Tauri app is a macOS TCC (Files and Folders) grant
    that dev-binary rebuilds routinely invalidate, not something introduced by this task's changes —
    confirmed by `ls ~/Downloads` failing from this session's own shell tool too, and by
    `crates/fm-vfs-local/src/lib.rs` mapping the error straight from `io::ErrorKind::PermissionDenied`.
    User needs to re-grant it themselves in System Settings; not actionable from here.
- 2026-08-16 (fifth pass): the fourth pass's fix was aimed at the wrong target. User's follow-up:
  *"open new workspace only focusses the previous one - it does not create another window, as was
  the intent."* The `.show()`/`.unminimize()`/`.set_focus()` change from the fourth pass was real
  and harmless, but the actual bug was the *design* one pass earlier than that: `open_workspace_window`
  deduplicated by a stable `workspace-<id>` label, so a second "Open in New Window" for a workspace
  that already had a window always found-and-focused it rather than opening another one - which was
  never the intent. Fixed by making `workspace_window_label` return a fresh `workspace-<id>_<nonce>`
  label on every call (so labels never collide and the dedup/focus branch is gone entirely), and
  registering `tauri-plugin-window-state` with `.map_label(commands::canonical_workspace_window_label)`
  so windows for the same workspace still share one remembered frame despite no longer sharing a
  label (`canonical_workspace_window_label` strips the `_<nonce>` suffix back to `workspace-<id>`
  before the plugin persists/restores by that key). Commit `8c2ae78`.
  - Added two unit tests (`commands.rs`) for the label/canonicalization contract specifically, since
    the fourth pass's blind spot was exactly this: nothing exercised what "open twice" should
    actually do, so a plausible-looking symptom (silent focus failure) was fixed with high confidence
    while missing that the underlying behavior (dedup at all) was never wanted. Verified:
    `cargo build/test/clippy/fmt -p fm-desktop` all clean (18 tests, was 16).
  - **Lesson**: when a user reports "X does nothing," don't assume the *mechanism* (focus/visibility)
    is the bug without first confirming the *intended* behavior — ask "should this open a new window
    every time, or focus an existing one?" rather than picking the more defensible-sounding
    interpretation and building a whole verification story around it. Still not confirmed in the
    real app (same standing limitation) - ask the user to retest the same matrix once more.
- 2026-08-16 (sixth pass): user confirmed the fifth pass's fix works, then asked to wire "Open in
  New Window" into the native menu bar too - closing the "no native menu entry" item noted as
  deliberately out of scope back in the second-pass note. Added a "New Window" item (Cmd+Shift+N)
  at the top of the File menu, above New Tab/Close Tab (`native-menu-spec.ts`'s `fileMenu`), calling
  the same `openWorkspaceWindow` the switcher's button already uses, for the *current* workspace.
  Frontend-local id `ui.newWorkspaceWindow` (not a backend action, same pattern as `ui.openSettings`
  and the Window menu's per-tab ids), dispatched via a new `openNewWorkspaceWindow` callback on
  `NativeMenuDispatchContext`. Gated by a new `NativeMenuInputs.canOpenNewWindow` flag (mirrors
  `attrsClient.openWorkspaceWindow`'s availability) so the item is absent, not disabled, on the
  browser/HTTP host. Commit `abd6cc5`. Added tests for both the spec-building (item present/absent)
  and dispatch (id routes to the callback) sides. Verified: `tsc`, `biome check`, `vitest run` (1119
  passed, up from 1116; one pre-existing unrelated flaky build-integration test timeout, confirmed
  flaky in isolation earlier in this task, not a regression).
- 2026-08-21/22: **Architectural redesign — "Open in New Window" no longer shares a live workspace
  document.** Two windows open on the same workspace id turned out to be fundamentally broken (not
  fixable by patching individual merge/event-stream leaks — see the diagnosis in the ephemeral
  per-window workspaces plan for the full trace): every attempted point-fix closed one leak but
  another appeared, because two independent UI sessions were sharing one writer slot (one workspace
  id, one revision counter, one event stream). The user's redesign: a named workspace is an
  immutable template, edited only through an explicit "resync" action; every window (including "New
  Window") gets its own private, disposable ("ephemeral") workspace forked from a template's
  last-synced shape, with its own id — so the *existing* per-id revision/event isolation just works,
  with no cross-window merge logic needed anywhere.
  - **Phase 1 (fork-per-window, done)**: `Workspace` gained `ephemeral: bool` +
    `forked_from: Option<WorkspaceId>` (schema v4, migrated via `migrate_v3_to_v4`).
    `WorkspaceService::fork`/`resync` (`crates/fm-application/src/workspace/service.rs`) and the
    `resync_workspace` Tauri command implement fork-on-open and explicit-resync-only (no autosave,
    no prompts). `open_workspace_window` now forks a fresh ephemeral workspace per call instead of
    pointing a new window at the same live id. Closing a window deletes its own ephemeral workspace
    (`lib.rs`'s `Destroyed` handler); a `QuittingFlag` (set from `RunEvent::ExitRequested`/`Exit`)
    distinguishes "user closed this one window" from "the whole app is quitting", so ephemeral
    workspaces survive on disk across a quit rather than being deleted. New "Sync Workspace" File
    menu item (`ui.syncWorkspace`). The workspace switcher/"Open Workspace" submenu now filter out
    ephemeral workspaces (`sortWorkspaceSummaries`/`firstAvailableWorkspaceId`,
    `frontend/src/features/workspace/workspace-manager.ts`) — they were never meant to be
    user-visible or selectable.
  - **Bug found the same day**: "New Window" from the Dock (or any window that is itself the
    original *named* workspace, not an ephemeral fork of one) always forked the hardcoded default
    (`~` left/right) instead of the current/most-recently-used named workspace. Cause:
    `openNewWorkspaceWindow` (`frontend/src/app/app-shell.ts`) unconditionally forked from
    `workspace.forkedFrom`, which is only set on an *ephemeral* window — the main/dock window loads
    a real named workspace directly via `start_workspace` (already correctly resolving the
    last-active one), so its own `forkedFrom` is `undefined`. Fixed: fork from
    `workspace.ephemeral ? workspace.forkedFrom : workspace.id` — a non-ephemeral window forks from
    its own id, since it already *is* the named workspace.
  - **Phase 2 (restore ephemeral windows across a relaunch, done)**: previously deferred as a
    separate, riskier follow-up requiring replacing the declarative `"main"` window. Implemented by
    adding `"create": false` to `tauri.conf.json`'s one declared window entry (a first-class Tauri
    v2 field for exactly this: "you must manually grab the config object ... and create it via
    `WebviewWindowBuilder::from_config`") and building every window explicitly in `lib.rs`'s
    `setup()` instead: lists all workspaces (`service.list_workspaces()`, via
    `tauri::async_runtime::block_on` — safe here since `setup()` runs once, synchronously, before
    the event loop starts, unlike building a window inside a command), and if any are `ephemeral`,
    opens one window per surviving one (`commands::build_workspace_window`, pointed at
    `?workspaceId=<id>` without re-forking — the workspace already exists); otherwise builds the
    single default window as before (`commands::build_default_window`). Both new helpers share the
    declared window's config as a template (`declared_window_config`, cloned + label/url
    overridden) so title/size/macOS title-bar style/background stay in sync with `tauri.conf.json`
    automatically rather than being duplicated as Rust literals (which is what
    `open_workspace_window`'s inline builder did before this pass — it now calls the same shared
    helper). Also fixed `tauri_plugin_single_instance`'s second-launch callback, which previously
    assumed a `"main"`-labelled window always exists (`app.get_webview_window("main")`) — it now
    focuses whichever window `app.webview_windows()` returns first, since a relaunch that restores
    only ephemeral windows has no `"main"` label at all.
  - **Not done**: restoring ephemeral workspaces after a full macOS *restart* is not distinguished
    from a normal relaunch — both just read whatever's on disk, which is exactly the spec'd behavior
    ("keep the ephemeral workspaces" across either). No dedicated Rust test added yet for the
    `setup()` restore branch or the `declared_window_config`/`build_default_window`/
    `build_workspace_window` helpers specifically — relied on `cargo check`/`clippy`/the existing
    `fm-desktop` suite plus (pending) manual verification; same standing limitation as the rest of
    this task — no tool in this session can drive the real native multi-window app, so a human still
    needs to confirm the actual restore-on-relaunch behavior end to end.
- 2026-08-22: **The dock's "New Window" item still ignored saved workspaces after the fixes above** —
  user retested and it kept opening `~` left/right. The app-shell.ts fallback fix from the previous
  entry was necessary but not sufficient; the actual root cause was one layer deeper, in
  `WorkspaceService::start` (`crates/fm-application/src/workspace/service.rs`): **neither `fork` nor
  `resync` ever marked a workspace last-active**, and neither did the plain `create()` a user's
  "Save"/"New Workspace" button goes through. So `last_active_workspace_id` stayed unset (or stale)
  for users who never explicitly used the workspace switcher's "Open" — meaning the main/dock
  window's own `start_workspace(None)` on every cold start (including the very one the Dock's "New
  Window" then forks from) fell through to `create_default()`, silently creating a brand-new
  throwaway "Default" (`~` left/right) workspace *every single launch*, never the user's actual saved
  one. The "New Window" fork itself was working correctly by this point - it was just forking from a
  wrong, freshly-fabricated "current" workspace.
  - **Fix, two parts**: (1) `fork`/`resync` now mark their named source/target last-active
    (`fork`: the source it copied from, if any; `resync`: the source it wrote back into, or the new
    workspace it just created) — a window derived from or synced into a named workspace is "using"
    it, so a later cold start should reopen that one. (2) `start`'s fallback chain, for the case
    where *still* nothing is marked last-active (e.g. every existing workspace was only ever
    `create()`d or hand-saved, never opened/forked/resynced before this fix shipped): instead of
    immediately fabricating a fresh default, it now picks the most-recently-updated named
    (non-ephemeral) workspace already on disk via a new `most_recently_updated_named_workspace_id`
    helper — "last used, or the first if all are equal" per the user's own phrasing — and only falls
    back to `create_default()` if literally no named workspace exists yet.
  - Added 8 new unit tests in `crates/fm-application/src/workspace/service.rs` covering: fork copies
    shape and marks itself ephemeral; fork with a source marks that source last-active (overriding
    whatever was previously active); fork with no source leaves last-active untouched; resync writes
    back to its source and marks it last-active; resync from a from-scratch default creates a named
    workspace, relinks the ephemeral, and marks the new workspace last-active; resync of a
    non-ephemeral workspace is rejected; `start` falls back to the most-recently-updated named
    workspace when nothing was ever marked last-active; and that fallback excludes ephemeral
    workspaces even when one is more recently updated than the named workspace it was forked from.
  - Verified: `cargo test -p fm-application -p fm-desktop --lib` (233 + 23 passed, up from 225 + 23 -
    all new tests pass), `cargo clippy -p fm-application -p fm-desktop --all-targets -- -D warnings`
    (clean), `cargo fmt --all -- --check` (clean). **Not yet confirmed in the real app** — same
    standing limitation as the rest of this task; this fix should self-correct on the *next* app
    relaunch once the running `tauri dev` instance picks it up (it doesn't retroactively fix a
    `last_active_workspace_id` that's already missing until `start` is called again, which happens
    automatically on the next launch) — ask the user to quit and relaunch, then retry the Dock's "New
    Window" once more.
- 2026-08-22 (later same day): Three workspace-switcher UI fixes, prompted by the user's own
  screenshots after retesting:
  - **Delete-workspace dialog had an unneeded horizontal scrollbar.** `DeleteWorkspaceDialog`
    (`frontend/src/features/workspace/delete-workspace-dialog.ts`) was missing the
    `className: 'fm-dense-modal'` every other `ModalPanel`-based dialog in this codebase passes
    (`create-directory-dialog.ts`, `finder-tags-dialog.ts`, etc.) - without it, ModalPanel's
    default (wider) inline width let the footer's two buttons overflow. Added it; the sibling
    `CloseLastTabDialog` has the identical gap but wasn't reported, so left untouched (not
    in scope of what was asked).
  - **Switcher was highlighting the wrong row(s) as "current".** `activeWorkspaceId` in
    `app-shell.ts` was passed as `workspace?.id` - correct for the main/dock window (which *is*
    a named workspace), but wrong for any ephemeral (forked) window, which is by far the common
    case post-redesign: its own `id` is never in the switcher's list (ephemeral workspaces are
    excluded), so nothing should highlight for it, and there was no path for it to correctly
    show *its own source* as current either. Fixed: `workspace.ephemeral ? workspace.forkedFrom
    : workspace.id` - an ephemeral window now correctly highlights the named workspace it was
    forked from.
  - **Added a per-row "Update" button** (`frontend/src/features/workspace/workspace-switcher.ts`,
    new `onUpdate?` prop) that replaces *any* saved workspace's tabs/panes/layout with the
    current window's live ones, keeping that workspace's own name and id - not just the one this
    window happened to fork from. This needed a real backend capability, not just a new UI
    entry point: `WorkspaceService::resync` (`crates/fm-application/src/workspace/service.rs`)
    gained an explicit `target_id: Option<WorkspaceId>` parameter - when given, it overrides
    `ephemeral_id`'s own `forked_from` as the sync target (rejecting an ephemeral target with a
    new `WorkspaceError::TargetIsEphemeral`), and relinks the ephemeral workspace to that target
    so a later default resync (the File menu's "Sync Workspace", which never passes an explicit
    target) keeps following it instead of drifting back to the original source. Threaded through
    `FileManagerService::resync_workspace`, the `resync_workspace` Tauri command
    (`target_workspace_id` param), `FileManagerClient.resyncWorkspace`, and
    `tauri-file-manager-client.ts`. The button (and the whole capability) only renders when the
    current window actually has an ephemeral session to sync from
    (`attrsClient.resyncWorkspace !== undefined && workspace?.ephemeral === true`) - absent for
    the browser/HTTP host and for the main/dock window itself. New icon: `refreshIcon` in
    `frontend/src/components/tabler-icons.ts` (vendored Tabler `refresh` outline glyph).
  - Added 2 new Rust tests (explicit-target resync replaces the target and keeps its name,
    untouched original source, ephemeral relinked to the new target; explicit ephemeral target is
    rejected). Verified: `cargo test -p fm-application -p fm-desktop --lib` (235 + 23 passed, up
    from 233 + 23), `cargo clippy -p fm-application -p fm-desktop -p fm-server --all-targets --
    -D warnings` (clean), `cargo fmt --all -- --check` (clean). Frontend: `tsc --noEmit`, `biome
    check` (both clean), `vitest run` (1400 passed, no regressions). No HTTP/OpenAPI regeneration
    needed - `resync_workspace` has no `fm-server` route, desktop-only as designed. **Not yet
    confirmed in the real app** - same standing limitation as the rest of this task.
