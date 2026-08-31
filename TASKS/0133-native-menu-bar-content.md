# 0133 Populate native menu bar content (macOS + Windows)

Status: done
Priority: high
Owner: unassigned
Agent: claude
Area: desktop
Depends on: 0059

## Context

`PlatformAdapter::install_native_menu` (`crates/fm-platform/src/adapter.rs`) is a hook-point-only
trait method. On macOS (0059) it acquires the `MainThreadMarker`, creates an `NSMenu`, and installs
it as the app's main menu via `NSApplication::sharedApplication().setMainMenu(...)` — but the menu
is **empty**. There is no File/Edit/View/Go/Window/Help structure, no OS-level `Cmd+,` Preferences
item, no populated Window menu (so Mission Control / Cmd+backtick window switching shows a generic
app entry instead of real menu items), and no dynamic "Open Recent" in the Dock menu. On Windows,
task 0131 now provides the HWND hook and this task supplies the equivalent HMENU content and
action routing.

Raised during a review of macOS integration gaps: fm's context menus and command palette (0051,
0052) cover in-app discovery well, but the OS-level menu bar — which macOS users expect to reflect
the app's capabilities and which text fields/inputs rely on for their built-in Edit menu wiring
(cut/copy/paste/undo working in native text fields) — is currently a no-op.

## Acceptance Criteria

- macOS: a real menu bar with standard sections — App menu (About, Preferences `Cmd+,`, Services,
  Hide/Quit), File (New window/tab, Close), Edit (Undo/Redo/Cut/Copy/Paste/Select All — wire to the
  same actions as 0049's action registry so behaviour matches the keyboard shortcuts already bound),
  View, Go (favourites/recent locations from 0070), Window (Minimize, Zoom, real window list), Help.
- Menu items that duplicate an existing action-registry command (0049/0050) dispatch through the
  same action id as the keyboard shortcut, not a separate code path — no divergent behaviour between
  pressing `Cmd+,` from the keyboard and clicking "Preferences…" in the menu.
- Windows: once 0131's hook lands, an equivalent `HMENU`-based menu bar with the same logical
  sections adapted to Windows conventions (File/Edit/View/Go/Window/Help, no separate App menu).
- The Window menu (macOS) or equivalent reflects actual open windows/workspaces, not a static list.
- "Open Recent" (or equivalent) reflects 0070's recent-locations list.
- Menu content updates when action availability changes (e.g. Undo disabled when there's nothing to
  undo), following whatever pattern 0052's context-menu availability checks already use.
- Tests: platform adapter unit tests asserting menu structure/item ids where feasible without a real
  windowing system; manual verification recorded for both platforms (native UI trees are hard to
  assert against in CI).

## Implementation Notes

- Reuse the action registry (0049) as the source of truth for menu item labels/shortcuts/enabled
  state rather than hand-maintaining a parallel list — the command palette (0051) already does this
  and is a good reference implementation.
- Keep menu construction behind the existing `PlatformAdapter` trait; don't leak `NSMenu`/`HMENU`
  types outside `fm-platform-macos`/`fm-platform-windows`.
- The Windows half depends on 0131's HWND hook point and is implemented behind the same platform
  adapter boundary; menu content remains opaque to the desktop host.

## Agent Notes

- 2026-08-15 claude: Implemented the macOS half end-to-end; Windows explicitly deferred (see
  below). Scope and two architectural decisions were confirmed with the user before implementation
  (not guessed): (1) macOS only in this task, Windows left as a documented gap pending 0131, which
  is still open/unstarted; (2) menu content/availability is computed entirely on the frontend
  (which already has tested availability logic in `frontend/src/features/commands/availability.ts`)
  and pushed to Rust as a plain spec — Rust never re-derives menu content, it only renders whatever
  tree it's handed. A third gap surfaced during implementation: the Edit menu's acceptance-criteria
  wording names Undo/Redo/Cut, none of which exist anywhere in this codebase (no undo-stack
  feature, no cut action) — confirmed with the user to omit them rather than invent placeholders;
  the Edit menu ships Copy/Paste/Select All only (native AppKit still gives Cut/Copy/Paste/Undo for
  free inside text fields via the standard responder chain). Preferences has no backend action
  either (`Cmd+,` was already frontend-only, calling `openSettingsDialog()` directly) — the native
  menu's Preferences item carries a synthetic `ui.openSettings` id routed to the same call, not a
  registry dispatch.
  - **fm-domain** (`crates/fm-domain/src/menu.rs`, new): `NativeMenuSpec`/`NativeMenu`/
    `NativeMenuItem`/`NativeMenuRole`, serializable, camelCase, pinned by explicit
    `serde_json::to_string` assertions (not just round-trip) since the frontend hand-writes
    matching TS types against this exact shape. `NativeMenuItem::Role` is a struct variant
    (`Role { role: NativeMenuRole }`), not a newtype — serde's internally-tagged representation
    can't nest a newtype's own unit-variant serialization under a field, so a newtype here would
    silently produce `{"kind":"role","quit":null}` instead of `{"kind":"role","role":"quit"}`; this
    was caught and fixed during integration (see below).
  - **fm-platform** (`adapter.rs`, `fallback.rs`, `Cargo.toml`): `install_native_menu` now takes
    `(&NativeMenuSpec, on_action: Arc<dyn Fn(String) + Send + Sync>)`; added an `fm-domain`
    dependency (layer 0 → layer 1, allowed by the architecture fitness test).
  - **fm-platform-macos** (`lib.rs`): real `NSMenu`/`NSMenuItem` construction from the spec via
    objc2's `define_class!` (a `MenuActionTarget` NSObject subclass handles every `Action` item's
    click through one process-wide callback slot — documented in-code as an honest design, since
    there is only ever one native menu bar per process). `Role` items get no callback at all: nil
    target routes them through the standard first-responder chain (`terminate:`, `hide:`,
    `orderFrontStandardAboutPanel:`, etc.), and `Services` registers its submenu via
    `NSApplication::setServicesMenu`. `key_equivalent` (KeyChord → key + `NSEventModifierFlags`
    bits) is factored out as a pure function specifically so it's unit-testable without a real
    windowing system — full `NSMenu` construction itself isn't (no windowing system in CI),
    matching the task's own acknowledgement of this limit.
  - **fm-platform-windows** (`lib.rs`): signature updated to match, still delegates to the fallback
    adapter (`Unsupported`) — no menu content, per the confirmed decision above.
  - **fm-application** (`service.rs`, `platform_mapping.rs`): thin `FileManagerService::
    install_native_menu` passthrough plus `map_native_menu_error`, following the exact convention
    already used by `file_icon`/`map_file_icon_error`.
  - **fm-desktop** (`native_menu.rs` new, `commands.rs`, `lib.rs`): two Tauri commands —
    `subscribe_native_menu_actions` (frontend subscribes a `Channel` once at startup) and
    `set_native_menu` (rebuilds the whole menu from a pushed spec, via the same
    `run_on_main_thread` + oneshot pattern already used by `start_native_drag`, since AppKit menu
    APIs require the main thread). No DTO mirror was added in `fm-transport-dto` — this isn't
    exposed over HTTP/OpenAPI, so the Tauri command takes `fm_domain::NativeMenuSpec` directly.
  - **Frontend** (`frontend/src/models/native-menu.ts`, `frontend/src/features/native-menu/*`,
    `app-shell.ts`): `buildNativeMenuSpec` (pure) builds the full menu tree from
    `registeredActions`/`favouriteActions()`/open tabs; `dispatchNativeMenuAction` routes incoming
    clicks to `openSettingsDialog()`, tab activation, or `actionCommandController
    .invokePaletteAction` (the same dispatch path the command palette already uses, satisfying the
    "no divergent behaviour" acceptance criterion). Menu sync uses a memoized `syncNativeMenu()`
    called from the existing state-mutation sites in `app-shell.ts` (not a genuine Meiosis service —
    confirmed with the user that `frontend/src/state/store.ts`'s Meiosis store isn't actually wired
    into `app-shell.ts`'s runtime, so this is the faithful equivalent: recompute on relevant change,
    skip the IPC call if the computed spec is unchanged from the last one sent). View menu content
    (no dedicated "view" category exists in the registry) uses the five sort-order toggle actions;
    Window menu flattens tabs across all panes with no pane-name prefixing — both judgment calls,
    called out here for future revisit rather than left silently undocumented.
  - **Integration fix**: two agents built the Rust and frontend halves in parallel against a fixed
    contract. The frontend caught a real bug in the originally-specified Rust `Role(NativeMenuRole)`
    newtype shape (documented above) and built against the corrected struct-variant shape; the
    fix was applied to `fm-domain` and `fm-platform-macos` during integration.
  - Tests (verified via targeted `cargo test`/`vitest run` invocations, not whole-suite totals):
    `fm-domain` 5 new (`menu::tests::*`), `fm-platform-macos` 2 new pure-function tests
    (`key_equivalent_*`), `fm-application` 1 new (`install_native_menu_forwards_the_spec_and_maps_
    adapter_failures`), `fm-desktop` 3 new (`native_menu_action_callback_*`,
    `native_menu::tests::has_no_subscription_until_one_is_set_then_returns_the_latest_one`),
    frontend 15 new (`native-menu-spec.test.ts` 11, `native-menu-dispatch.test.ts` 4). Full affected
    suites also re-run and green: `fm-domain`/`fm-platform`/`fm-platform-macos`(27 passed, 1
    pre-existing ignored)/`fm-platform-windows`/`fm-application`/`fm-desktop` lib and integration
    tests, the `fm-test-support` architecture fitness test (confirms the new `fm-platform` →
    `fm-domain` dependency respects crate layering), and the full frontend `vitest run` (1112
    passed) plus `tsc --noEmit` (clean). `cargo clippy --all-targets` clean across every touched
    crate (fixed a `missing_docs`, a `type_complexity` on the process-wide callback static, two
    `doc_lazy_continuation` formatting issues, and one `cloned_ref_to_slice_refs` along the way). A
    `copy_directory_operation` integration test flaked once under heavy concurrent build load
    (timing-sensitive, unrelated file) and passed cleanly in isolation on retest.
  - **Known gap**: manual visual verification of the actual running macOS menu bar was not
    performed by the agent — this sandboxed session has no screen-recording/automation permission
    (`osascript`/`screencapture` calls hang on a permission prompt with no way to grant it
    non-interactively), confirmed with the user, who will do a `pnpm run dev:tauri` visual check
    themselves. Everything short of that visual check (compilation, unit/integration tests,
    lint, architecture fitness, an actual `fm-desktop` boot test asserting the Tauri runtime starts
    with the new commands registered) is green.
  - **Follow-up**: Windows menu content (task 0131, still open/unstarted).
- 2026-08-15 claude: The user ran the macOS build themselves (the manual-verification step flagged
  above) and reported four real bugs from an actual screenshot of the running menu bar. All four
  are fixed and covered by the automated checks below; none required design changes.
  1. **Go menu dumped every registered action** (~60 items, scrolling off-screen, duplicating Edit,
     including irrelevant items like "extend selection") instead of just favourites. Root cause:
     `app-shell.ts`'s `syncNativeMenu()` passed `actionsWithFavourites()` (registered actions *plus*
     favourites) as `buildNativeMenuSpec`'s `favouriteActions` input, not the plain `favouriteActions()`
     synthetic list — a naming collision between the local function and the input field. The pure
     `buildNativeMenuSpec` function itself was already correct and already tested for this
     ("builds the Go menu from favourite actions, not the plain registered actions"); only the
     call site was wrong, which is why the existing test suite didn't catch it. Fixed by passing
     `favouriteActions()` instead.
  2. **File/Edit/View menu clicks did nothing.** Root cause: a startup race. `set_native_menu`
     binds whichever click-callback channel is *currently* subscribed on the Rust side at call
     time; `subscribe_native_menu_actions` and the very first `syncNativeMenu()` push both fire
     from `oninit` as independent promises with no ordering guarantee. If the first menu push won
     the race, the backend installed the menu bound to a no-op callback (nothing subscribed yet)
     - and since nothing about the spec's *content* changes just because the subscription later
     completes, the memoized diff in `syncNativeMenu()` never re-pushed, leaving every click
     permanently wired to nothing. Fixed with an explicit `nativeMenuChannelReady` flag, set only
     once `subscribe_native_menu_actions` actually resolves and gating `syncNativeMenu()` entirely
     until then, guaranteeing the first successful push always carries the real callback.
  3. **Every View-menu sort item showed the identical "^F" shortcut** and none of them worked as
     shortcuts either. Root cause: `fm-platform-macos`'s `key_equivalent` took only the *first
     character* of the `KeyChord.key` string; `"F3"`..`"F7"` (the five sort actions' real shortcuts)
     all start with `'F'`, so every one collided onto the same displayed (and functionally wrong)
     `Ctrl+F` key equivalent. The doc comment claimed multi-character keys were "left untranslated"
     but the code didn't actually do that. Fixed to return a blank key equivalent for any
     multi-character key name instead of truncating it - caught an existing test
     (`key_equivalent_reports_no_modifiers_for_a_plain_chord`) that had enshrined the buggy
     first-character behavior as correct (`"Escape"` → `"e"`); fixed that test and added
     `key_equivalent_leaves_multi_character_key_names_blank_instead_of_colliding` covering all five
     sort shortcuts plus Escape/Enter/Tab.
  4. **App menu showed "fm-desktop" instead of "Procyon."** Not actually a code bug: AppKit always
     replaces the App menu's displayed title with the process name, and `cargo tauri dev` runs the
     raw unbundled binary (no `.app`/`Info.plist` for `CFBundleName` to come from), so the OS falls
     back to the executable name. Fixed anyway by having `install_native_menu` call
     `NSProcessInfo::processInfo().setProcessName(...)` using `spec.menus[0].title` - repurposing
     the App menu's already-supplied (and until now AppKit-ignored) title for exactly this. The
     frontend's `appMenu()` title changed from the placeholder `'fm'` to `'Procyon'` to match the
     title bar label already used elsewhere in `app-shell.ts`.
  - Verified: `fm-platform-macos` lib tests (28 passed, 1 pre-existing ignored, including the new
    regression test), `cargo clippy -p fm-platform-macos --all-targets -- -D warnings` clean, full
    frontend `vitest run` (1112 passed) and `tsc --noEmit` clean.
  - **Still not independently re-verified by the agent**: the four fixes above address the reported
    symptoms with clear, confirmed root causes, but the actual rebuilt menu bar has not been
    re-screenshotted by a human yet (same screen-recording/automation limitation as before). Also
    unverified: whether other Edit/File items beyond what was screenshotted work correctly, and
    whether the Go-menu/click-dispatch fixes fully resolve "most functions do not work" or only the
    specific failures identified - recommend another `pnpm run dev:tauri` pass.
- 2026-08-15 claude: Separately, added `.claude/settings.json` (project-level, committed) with a
  `PreToolUse` hook that auto-extends the Bash tool timeout to 15 minutes for any `git commit`/`git
  push` command, since this repo's pre-commit/pre-push hooks routinely exceed the default 2-minute
  timeout and were repeatedly stalling agents. Not part of this task's scope, but landed alongside
  it at the user's request after repeated commit timeouts during this same session.
- 2026-08-15 claude: A second manual-testing round surfaced three more real bugs, all fixed.
  1. **View menu sort items did nothing.** Root cause: `core.sortByName`/`Extension`/`Date`/`Size`/
     `Unsorted` have no backend effect - like `core.preferences`, sorting is entirely frontend-owned
     workspace view state, applied via `GlobalKeydownContext.setSort` from the Ctrl+F3..Ctrl+F7
     keydown handler, never through `invoke_action`. Routing these ids through the generic
     `invokePaletteAction` path (as every other menu item does) was therefore always a no-op.
     Fixed by exporting the keydown handler's `SORT_SHORTCUT_DESCRIPTORS` mapping (single source of
     truth, not duplicated) and adding a dedicated case in `dispatchNativeMenuAction`: a sort id
     resolves the active pane and calls a new `setSort` context field, which app-shell.ts wires to
     `globalKeydownHandlerContext.setSort` - the exact same call the keyboard shortcut makes.
  2. **Go menu's "Open favourites" opened the Command Palette instead of navigating.** Not a bug in
     what `core.favourites` does (that's its correct, intentional behaviour when invoked from the
     palette/keyboard - open the palette pre-filtered to favourites) but a bad fit for a native
     menu, which already lists every individual favourite as its own `core.favourite.<index>` item
     immediately below. Fixed by excluding `core.favourites` itself from the Go menu's items in
     `goMenu()` - the menu itself is the favourites browser, it doesn't need a launcher for one.
  3. **Switching tabs via the Window menu unexpectedly selected the first file.** First confirmed
     with the user that normal in-app tab-bar clicks do *not* do this (so this was not the
     already-known, out-of-scope, pre-existing "first visit to a tab defaults its cursor to the
     first entry" behaviour in `updatePane` - that stays untouched). The actual cause: the Window
     menu lists every open tab per pane (not just each pane's active one), so clicking an item that
     represents a pane's *already*-active/displayed tab - the common case when using the Window
     menu purely to switch keyboard focus to another pane - hit `tabController.activateTab`'s
     "re-click the same tab" branch. That branch triggers a background directory reload without
     ever updating `activePaneId` (so focus never actually moved) and the reload's `updatePane`
     call is what disturbed the pane's selection. Fixed `activateTabByKey` in app-shell.ts to check
     for this case first and call the existing `activatePane` helper instead (a lightweight,
     no-reload "just switch focus" operation already used by `selectTab`'s own re-click handling) -
     only a genuine cross-tab switch still goes through `tabController.activateTab`.
  - New tests: `native-menu-dispatch.test.ts` gained three sort-dispatch cases (resolves the
    active pane, `core.sortUnsorted` sends an empty sort, no-ops with no active pane) and
    `native-menu-spec.test.ts`'s Go-menu test was corrected plus a new case added asserting
    `core.favourites` itself never appears. Verified: `npx tsc --noEmit` clean, full frontend
    `vitest run` (1116 passed, up from 1112).
  - Follow-up: the user separately confirmed the normal in-app tab bar *also* auto-selects the
    first entry the first time a tab's directory is (re)loaded with no prior cursor - not native
    menu specific, so initially logged as out of scope above. The user then asked for it to be
    fixed anyway, for every trigger (mouse, Ctrl+Tab, and the native menu): landing somewhere
    should position the keyboard cursor, never also select an entry - selecting is a deliberate
    user action. Fixed in `updatePane` (app-shell.ts): the auto-positioning branch now always sets
    `selectedEntryIds: []`, only ever setting `cursorEntryId`/`anchorEntryId`. Verified this
    doesn't regress single-selection keyboard actions like F3/`core.view`, which already act "on
    the cursor file regardless of the wider selection" per its own comment in
    `global-keydown-handler.ts` - the app has multiple `getSelectedEntriesOrCursor` call sites
    that already treat "cursor with no selection" as a normal, handled state. Three existing tests
    had encoded the old auto-select behaviour as an explicit, deliberate guarantee (with their own
    "so single-selection actions work immediately" rationale) - now stale given `core.view`'s later
    cursor-only handling - updated to assert `.fm-selected-row` is absent instead. Verified:
    `tsc --noEmit` clean, full `vitest run` green (1115 passed; the one other failure,
    `config/mithril-inspector.test.ts`, is an unrelated pre-existing timeout flake, confirmed by
    passing cleanly in isolation).
- 2026-08-15 claude: User confirmed via manual testing (`pnpm run dev:tauri`) that the macOS menu
  bar and all reported fixes work as expected. Marking done. Windows half remains explicitly out
  of scope, deferred to task 0131 (still open/unstarted) per this task's own acceptance criteria.
- 2026-08-20 claude: Implemented the Windows half after task 0131 landed. `WindowsPlatformAdapter`
  now recursively renders the frontend-provided `NativeMenuSpec` into top-level `HMENU` menus and
  nested popup menus using `AppendMenuW`/`CreatePopupMenu`. Action items receive stable per-install
  command ids, preserve enabled/checked state, and display Windows-style `Ctrl`/`Alt`/`Shift`
  shortcut labels. Role items use Windows labels (`Exit`, `Maximize`, `Hide`, etc.) and do not route
  through the application callback. A per-window subclass handles `WM_COMMAND`, maps command ids
  back to action ids, invokes the existing callback channel, and delegates unrelated messages to
  the original window procedure. Reinstalling a menu replaces the stored action map and callback.
  - Added pure unit tests for shortcut-label formatting and Windows role labels. The full
    `fm-platform-windows` suite (14 tests) and strict clippy pass on Windows.
  - Manual visual verification of the running Tauri menu bar remains outstanding; this Windows
    implementation is compile- and unit-tested here, but has not been exercised through an
    installed desktop binary on a separate Windows machine.
