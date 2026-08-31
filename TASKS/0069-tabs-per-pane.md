# 0069 Tabs per pane

Status: done
Priority: medium
Owner: unassigned
Agent: unassigned
Area: frontend
Depends on: 0080, 0050

## Context
`file-manager-coding-agent-spec.md` §16 milestone 3, §5.3 (`PaneState` already holds a list of tabs)
and §37.

## Acceptance Criteria
- `Ctrl/Cmd+T` opens a new tab in the active pane at the current location; `Ctrl/Cmd+W` closes the
  active tab (never the last one without confirmation).
- Tab strip shows each tab's directory name with a tooltip of the full path, supports reordering by
  drag, and shows an overflow affordance when tabs exceed the width.
- Each tab keeps its own location, navigation history, sort, filter, cursor and selection.
- Switching tabs is instant: the previous snapshot is reused if still valid, otherwise refetched,
  and pending requests for a hidden tab are cancelled.
- Tabs persist across restarts via the `AddTab`/`CloseTab`/`ActivateTab` workspace commands (0080),
  including per-tab history.
- Keyboard: cycle tabs, jump to tab N, reopen last closed tab.
- Vitest tests: tab lifecycle, per-tab state isolation, persistence round-trip.

## Implementation Notes
- The backend tab model was refined by 0078 (renamed/extended `TabState`) and gained a real
  command surface in 0080 (`AddTab`/`CloseTab`/`ActivateTab`) — this task consumes that surface
  rather than inventing its own persistence, so it should need no further domain redesign; if it
  does, that is a signal worth recording in the notes.
- Directory watchers must be released for tabs that are closed (0020).

## Agent Notes
- Not started.
- 2026-07-31 codex: This task is a prerequisite for 0084. Keep tab lifecycle and rendering reusable
  across workspace switches, and make pending-update flush/subscription cleanup explicit so the
  later workspace manager does not need to recreate tab behavior.
- 2026-08-02 copilot: Re-verified a substantial, high-quality implementation left uncommitted by a
  prior session (same pattern as 0068) against every acceptance-criteria line, fixed two concrete
  gaps found during that audit, and closed it out.
  - **Re-keying design**: all per-tab runtime state in `frontend/src/app/app-shell.ts` (9 module
    Maps: `directories`, `selections`, `metadataLoaders`, `metadataViews`, `sortedEntries`,
    `sortRequests`, `quickFilterDrafts`, `quickFilterOpen`, `filteredEntries`) and in
    `frontend/src/features/navigation/navigation.ts` (`activeRequests`, `paneViews`) is keyed by a
    composite string `` `${paneId}:${tabId}` `` via a small `tabKey`/`activeTabKey` helper duplicated
    in both files — verified every one of the 11 Maps was actually converted, not partially.
    `NavigationController.abort(paneId, tabId)` is called from `activateTab`/`clearTabState` before
    loading the newly-activated tab, which is what cancels pending requests for a hidden tab
    (covered by `navigation.test.ts`'s "abort() cancels one tab in flight without touching its
    sibling").
  - **Tab strip / drag reorder / close confirmation**: tab strip UI lives in
    `frontend/src/features/panes/pane.ts` (keyed vnodes per tab and `title` attr for the full-path
    tooltip). Dragging now dispatches the atomic `MoveTab` workspace command, making same-pane
    reordering persistent and allowing a complete tab (history, view and transient state included)
    to move between panes. The frontend re-keys its tab-scoped directory, selection, filter, viewer,
    disk-usage, terminal and navigation state after a successful cross-pane move. Moving a pane's
    only tab creates the normal home-directory replacement in the source pane. The tab strip scrolls
    (`overflow-x: auto`) but has no dedicated fade/arrow overflow affordance beyond that — accepted
    as a reasonable minimal reading of "shows an overflow affordance". Closing a pane's only tab is
    gated behind a new
    `close-last-tab-dialog.ts` confirmation (`requestCloseTab`'s `tabOrder.length <= 1` check);
    confirming/cancelling is covered end-to-end by new `app-shell.test.ts` integration tests.
  - **Keyboard cycle/jump/reopen**: `core.nextTab` (Ctrl+Tab), `core.previousTab`
    (Ctrl+Shift+Tab) and `core.reopenClosedTab` (Ctrl+Shift+T) were added to
    `crates/fm-application/src/action.rs`; digit jump-to-tab-N is handled directly in
    `handleGlobalKeydown` (not through the action registry) for Ctrl+1..9. Checked for collisions:
    `core.switchPane` uses bare `Tab`/`Shift+Tab` (no Ctrl) so it never matches; `core.newTab`
    (Ctrl+T) and `core.reopenClosedTab` (Ctrl+Shift+T) don't collide because of the Shift flag.
    `git diff --stat` against `frontend/openapi/openapi.json` for this change is empty, and
    `ActionDescriptorDto` is a generic shape that doesn't enumerate action ids, so no OpenAPI/Orval
    regeneration was needed.
  - **Directory-watcher-on-tab-close (0020)**: confirmed there is no per-tab watch subscription in
    `crates/fm-application/src/directory.rs` — watches are ref-counted by `Location`
    (`WatchHub`/`SharedWatch`), and each pane only ever has one active `PaneRequest` whose
    `watch_cancellation` is released whenever a new `list()` targets a different location than the
    previous one for that pane (already exercised by the pre-existing
    `repeated_navigation_releases_superseded_watch_registrations` and
    `loading_another_page_keeps_the_directory_watch_registered` tests). Since `activateTab`/
    `closeTab` ultimately drive the pane through the same `list()` path, closing or switching tabs
    correctly releases the watch for a location no longer shown by any tab — no gap here.
  - **Concrete gaps found and fixed** (small, targeted):
    1. `fixtures/mock-responses/actions.json` — the hand-maintained mock-client action fixture was
       never updated with the 3 new core actions, so under the `'mock'` runtime (used by nearly all
       frontend tests, and the default dev/demo runtime) Ctrl+Tab/Ctrl+Shift+Tab/Ctrl+Shift+T were
       completely unreachable — `dispatchKeybinding` had nothing to match against. Added the three
       descriptors (mirroring `action.rs`'s ids/titles/shortcuts) and updated the corresponding
       hardcoded id-list assertion in `mock-file-manager-client.test.ts`. This is the same class of
       bug as the `BackendEventPayload` allowlist gap from 0068: a second, hand-maintained data
       source that mirrors a backend registry and must be kept in sync by hand.
    2. `frontend/src/keybindings/dispatcher.ts`'s `BROWSER_RESERVED` set had `CTRL+P`/`CTRL+W` but
       was missing `CTRL+T`, even though real browsers also reserve Ctrl+T for opening a new browser
       tab. Added it, with a new regression test in `dispatcher.test.ts` (mirroring the existing
       Ctrl+P coverage: unavailable under `runtime: 'browser'`, available under `runtime: 'desktop'`
       — desktop/Tauri and the `'mock'` test runtime are unaffected).
  - Added a new `describe('tabs per pane (task 0069)', ...)` block to `app-shell.test.ts` (6 tests)
    covering: opening a tab with Ctrl+T at the active tab's current location; closing a non-last tab
    directly with Ctrl+W; gating close-to-zero behind the confirmation dialog; cancelling the
    dialog; cycling with Ctrl+Tab/Ctrl+Shift+Tab and jumping with Ctrl+1; reopening the most
    recently closed tab with Ctrl+Shift+T. Two mithril-materialized/Vitest gotchas surfaced while
    writing these (worth knowing for future tab/dialog tests in this codebase): `ModalPanel` always
    renders its `role="dialog"` markup for every dialog present in the tree, open or not (visibility
    is `aria-hidden`/CSS only, not conditional mounting) — a bare `root.querySelector('[role=
    "dialog"]')` is ambiguous once more than one dialog exists, so tests need to disambiguate by
    content/scope button lookups to the specific dialog; and `expect.anything()` never matches an
    explicit `undefined` argument (the client's optional trailing `signal` param), so assertions on
    calls with an omitted signal must match `undefined` literally.
  - Final counts: frontend `48` test files / `346` tests passing (up from 339 pre-audit: +6
    app-shell + 1 dispatcher); `tsc --noEmit` clean; `pnpm run lint:frontend` (Biome) clean;
    `cargo test -p fm-application`, `cargo clippy -p fm-application --all-targets -- -D warnings`,
    and `cargo fmt --all --check` all clean.
  - Left `frontend/src/api/client/tauri-file-manager-client.ts` (a pre-existing, unrelated one-line
    import-type change already present on disk before this task) and commit `d8a1f68` untouched.
