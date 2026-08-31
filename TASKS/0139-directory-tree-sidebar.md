# 0139 Directory tree dialog / sidebar tree view

Status: done
Priority: medium
Owner: unassigned
Agent: claude
Area: frontend
Depends on: 0129

## Context

Split out of [0129](0129-total-commander-shortcuts-major-features.md) (Alt+F10 / Ctrl+F8 row) in the
2026-08-14 re-triage — confirmed still genuinely missing, not just undiscovered. Total Commander's
Alt+F10/Ctrl+F8 open a directory tree (dialog or sidebar) for fast hierarchical navigation and
jump-to. fm's panes today only render a flat listing of the active directory (0024's virtualized
table) plus the favourites menu (0070) for jumping to saved locations — there is no way to see or
navigate the surrounding directory structure without opening each level in turn.

Meaningfully sized (a new component, not a shortcut binding), and a commonly-expected feature in a
"state-of-the-art" file manager (Finder's sidebar, Explorer's tree pane, most dual-pane managers).

## Acceptance Criteria
- A tree view (sidebar or toggleable dialog — pick one; a persistent sidebar is more discoverable
  and more consistent with Finder/Explorer, a dialog is cheaper to build and closer to TC's actual
  behaviour) showing the directory hierarchy from the active pane's provider root downward.
- Lazy expansion: child nodes are fetched only when a branch is expanded, not eagerly for the whole
  tree (reuse the existing `VfsProvider`/`DirectoryService` listing path, not a bespoke walk).
- Selecting/activating a tree node navigates the active pane to that directory, and the tree stays
  in sync when the active pane navigates by other means (breadcrumbs, history, favourites) — the
  tree's expanded/highlighted path always reflects the active pane's current location.
- Works across VFS providers (local, SFTP, FTP, archive, etc.), not just the local filesystem —
  reuse the same provider abstraction the table view already depends on.
- Keyboard-navigable (arrow keys to expand/collapse/move, matching the accessibility bar set by the
  rest of the app per 0066).
- Tests: lazy-expansion fetch behaviour, active-pane-location sync in both directions, provider
  parity (at least local + one remote provider), keyboard navigation.

## Implementation Notes
- Check [0134](0134-thumbnails-and-grid-view.md) before starting — it introduces a general
  view-mode/layout question for panes; keep the tree view's layout integration aware of whatever
  that task lands, even though they're separate UI surfaces (tree sidebar vs. grid/icon view).
- Favour reusing 0024's virtualized list primitives for rendering large flat runs of siblings within
  an expanded node, rather than a fully custom tree-rendering component, if that keeps the
  implementation simpler.
- Decide sidebar vs. dialog early — this materially changes the pane layout work involved (0026)
  vs. a self-contained modal.

## Agent Notes
- 2026-08-18 claude: Implemented as a persistent sidebar (chose sidebar over dialog per the
  acceptance criteria's own steer — more discoverable, more consistent with Finder/Explorer — and
  because 0134 already established a per-pane toggle-button precedent in the breadcrumb row, not a
  modal). Built end-to-end via TDD.

  **Backend**: `DirectoryService::list_children` (`crates/fm-application/src/directory.rs`) is a
  new, deliberately *pane-unbound* listing method, reusing the same `list_all`/`ProviderRegistry`
  primitives as `DirectoryService::list` but keeping none of `list`'s per-`PaneId`
  cancellation/revision/watch state. This was a necessary deviation from a literal reading of "reuse
  the existing `DirectoryService` listing path": `list()`/`navigate()`/`refresh()` are all keyed by
  `PaneId` and *cancel the previous in-flight request for that pane* on every call — calling them
  with a tree node's location under a synthetic or reused `PaneId` would race with or cancel a
  pane's own active listing. `list_children` reuses the underlying provider-listing primitives
  instead, which is the part of "the existing listing path" that actually generalizes; a regression
  test (`list_children_does_not_disturb_a_pane_s_own_in_flight_listing` in
  `crates/fm-application/tests/directory_tree.rs`) locks this in. New `ListDirectoryChildrenRequest`
  DTO (`crates/fm-transport-dto`, no `paneId`/`workspaceId`), `FileManagerService::
  list_directory_children` wrapper (returns `Vec<EntrySummaryDto>`, reusing the existing entry DTO
  rather than inventing a leaner one, to avoid a second entry-shape/conversion path), new route
  `POST /api/v1/directories/children` (`apps/fm-server/src/routes/directory.rs`, same
  `require_within_roots` guard as the sibling directory routes) and Tauri command
  `list_directory_children` (registered in both `invoke_handler` blocks in
  `apps/fm-desktop/src-tauri/src/lib.rs`). OpenAPI document and generated TS client regenerated via
  `pnpm run api:export && pnpm run api:generate`.

  **Frontend**: `FileManagerClient.listDirectoryChildren(location, showHidden, signal?)` added to
  all three adapters (http/tauri/mock — the mock adapter reads the same `directories.json` fixture
  used by `listDirectory`, filtered to directory-kind entries). New `frontend/src/features/
  directory-tree/` module:
  - `directory-tree-state.ts` — pure logic: `TreeChildrenState` (expanded set + children cache +
    loading/error maps, held as an ordinary immutable value, the same pattern as `terminal-state.ts`'s
    open-drawer set — not routed through the central meiosis/mergerino store, since it's UI-local
    and ephemeral), `flattenVisibleTree` (recursively flattens only *cached and expanded* nodes into
    a linear row list — nothing beyond a collapsed/unfetched node is ever computed, which is what
    makes lazy expansion lazy), and `ancestorChain` (root-to-target directory chain for the
    active-pane sync, built on the existing `parentLocation` from `navigation.ts`).
  - `tree-keybindings.ts` — `interpretTreeKey`, a DOM-free pure interpreter mirroring `selection/
    keybindings.ts`'s `interpretSelectionKey` shape, implementing the standard WAI-ARIA `role="tree"`
    keyboard pattern (Right expands/descends, Left collapses/ascends, Up/Down/Home/End move focus,
    Enter/Space activate).
  - `directory-tree.ts` — the `DirectoryTree` Mithril component (`role="tree"`/`"treeitem"`,
    `aria-expanded`/`aria-selected`/`aria-level`/`aria-activedescendant`, roving virtual focus).
    Presentational only (state and fetching live in the caller, mirroring how `DirectoryTable`
    delegates to `Pane`). Reuses `directory-table/windowing.ts`'s `calculateVisibleWindow`/
    `scrollOffsetForIndex` over the flattened row list exactly as the Implementation Notes suggested,
    rather than a bespoke tree renderer — verified with a 500-node windowing test asserting only a
    bounded subset of rows is actually mounted.
  - Wired into `app-shell.ts`: `treeState`/`treeSidebarOpen`/`treeRootLocation` as ordinary closure
    state (same shape as the existing `openTerminalLocations` pattern), mounted as a `.fm-directory-
    tree-sidebar` flex column beside `WorkspaceLayoutView` inside `main.fm-workspace` (`main.fm-
    workspace` gained `display: flex`, `.fm-workspace-layout` gained `flex: 1 1 auto` so it still
    fills the remaining width - no change to the pane-split/`WorkspaceLayout` model itself).
    Root computed via the existing `rootLocationFor` (task 0128) rather than a new helper, accepting
    its documented "not fully provider-aware for a remote provider's configured start path" caveat
    as a known limitation rather than solving it here (also true of `rootLocationFor`'s existing
    Ctrl+Backspace caller). Two-way sync is driven by `syncDirectoryTreeToActiveLocation()`, called
    once per render (a single string comparison against the last-synced location URI when nothing
    changed) rather than hooked into `navigate()`'s `onLocationVisited` callback specifically -
    deliberate, since a tab switch or pane switch changes the active location without going through
    `navigate()` at all, and the render-time diff catches every path uniformly. Toggle is Alt+F10
    (Total Commander parity; Ctrl+F10 was already taken by `core.clearQuickFilter`), added as a new
    `GlobalKeydownContext.toggleDirectoryTree()` special-cased in `global-keydown-handler.ts` the
    same way the embedded-terminal toggle already is (a pure UI toggle, not routed through the
    backend action registry).

  **Verified**: `cargo test --workspace`, `cargo clippy --workspace --all-targets` (zero warnings),
  `cargo fmt --all --check` all clean from the repo root. New backend tests, verified by running
  exactly these targets: 4 in `crates/fm-application/tests/directory_tree.rs` (directories-only
  filtering + sort, hidden-file filtering, the pane-non-interference regression above, and archive-
  provider parity using a real in-memory-built zip fixture, not just the local provider), 1 in
  `fm-application/src/service.rs` (`list_directory_children` wrapper), 1 new in `apps/fm-server/
  tests/directory_routes.rs` (the new route's happy path) plus that file's existing
  stable-operation-id contract test extended to cover it. Frontend: `pnpm exec tsc --noEmit` clean;
  `biome check .` clean (the same 3
  pre-existing `noDescendingSpecificity` warnings noted in earlier tasks, none in files this task
  touched). New/changed frontend tests, verified by running exactly these files: 14 in `directory-
  tree-state.test.ts`, 12 in `tree-keybindings.test.ts`, 10 in `directory-tree.test.ts`, 1 new in
  `http-file-manager-client.test.ts`, 2 new in `mock-file-manager-client.test.ts`, 1 new in
  `global-keydown-handler.test.ts` (plus the existing suite's mock-context update), 2 new in
  `app-shell.test.ts` (Alt+F10 toggle + click-to-navigate, and reverse sync from a table `Enter`
  navigation). Full `pnpm exec vitest run`: 1339/1339 passing,
  zero flakes. Manual browser verification (mock runtime) was attempted but blocked by an unrelated
  dev server already holding the configured port in this environment (not something safe to resolve
  by killing another session's process); relied instead on `app-shell.test.ts`'s full-DOM,
  real-`AppShell`-mounted integration tests (keyboard dispatch, click, and `vi.waitFor` assertions
  against actual rendered ARIA state) for end-to-end confidence, which is a materially stronger
  signal than a shallow component test but is not the same as eyes-on-screen confirmation - flagged
  here honestly rather than claimed as done.

  **Known gaps / explicitly deferred**: tree root for remote providers uses `rootLocationFor`'s
  URI-prefix heuristic rather than the connection-aware `remoteRootLocation` (a configured non-`/`
  start path lands one level higher than intended) - matches an existing, already-accepted
  limitation elsewhere in the codebase, not a new one. No dedicated SFTP/FTP-over-the-wire test (the
  provider-parity test uses the archive provider, which - like SFTP/FTP - is a real non-local
  `FileSystemProvider` exercising the same `ProviderRegistry` dispatch, but isn't network I/O);
  reusing `crates/fm-application`'s existing `ssh_sftp_operations.rs`-style live-server test fixture
  for `list_children` specifically would be a reasonable follow-up. Type-ahead-select within the tree
  was not added (only arrow/expand/collapse/activate navigation, per the acceptance criteria's
  explicit list) - a plausible small follow-up mirroring `TypeaheadController` if wanted later.

- 2026-08-19 claude: Follow-up pass on user-reported UX polish after manual testing.

  **Frontend only, no backend changes.**
  - **Focus-on-open + Page Up/Down**: `DirectoryTree` gained `registerFocus` (mirroring
    `TerminalDrawer`'s pattern), called by `app-shell.ts`'s `toggleDirectoryTree()` via
    `requestAnimationFrame` right after opening, so arrow keys work without an extra click.
    `tree-keybindings.ts`'s `interpretTreeKey` now maps PageUp/PageDown to `moveFocus` with a
    ±10 offset (matching `pane.ts`'s own fixed page size, not a viewport-height computation);
    `directory-tree.ts` clamps the resulting index instead of no-op'ing past the list's ends.
  - **No focus ring, dimmed originating pane**: `.fm-directory-tree` now sets `outline: none`
    (matching `.fm-pane`'s convention - focus is conveyed by the focused/selected row instead).
    Discovered via investigation that an inactive pane's cursor row previously had *no* visual
    marker at all when a *different* pane held `workspace.activePaneId` - not something specific
    to the tree. Fixed properly: `theme.css`'s filled/tinted cursor-row rules now additionally
    require `:focus-within` on `.fm-pane[data-active="true"]`, so a pane that is still the
    "active" pane for command routing but no longer holds real DOM focus (because focus moved to
    the tree sidebar, or - as a side effect - to the other pane) falls through to the existing
    unconditional outline-only `box-shadow` rule, the same "not focused, still showing the
    cursor" treatment the grid view's cursor tile already used.
  - **Tab/Shift+Tab 3-way loop**: `WorkspaceLayoutViewAttrs` gained `onPaneCycleBoundary?: () =>
    boolean`, checked in `workspace-layout.ts`'s existing pane-to-pane Tab handler only when Tab
    would wrap past the first/last pane in the split (not on every Tab) - if it returns `true`
    (tree open, focus redirected there), the normal wrap is skipped. Leaving the tree is handled
    entirely inside `DirectoryTree` itself (a new `moveFocusOut` `TreeKeyCommand`, since the tree
    sits outside any pane and nothing else is listening for its Tab): Tab focuses
    `workspace.paneOrder[0]`, Shift+Tab focuses the last pane, both via the existing
    `globalKeydownHandlerContext.focusPane`.
  - **Root row chrome**: root now renders no expand-toggle button at all (saves horizontal space
    - it has nothing to collapse into). Non-root rows' expand/collapse glyph changed from a plain
    text triangle character to two new vendored Tabler icons (`chevronRightIcon`/
    `chevronDownIcon` in `components/tabler-icons.ts`, matching the existing
    `trustedStrokeIcon` vendoring convention exactly); the loading state changed from a `'…'`
    text glyph to a small CSS spinner (`.fm-tree-loading-spinner`).
  - **Close button**: `app-shell.ts` now wraps `DirectoryTree` in a `.fm-directory-tree-sidebar`
    with a small `.fm-directory-tree-header` above it (title + a `closeIcon` button calling the
    same `toggleDirectoryTree()` used by Alt+F10), rather than adding sidebar chrome to the
    presentational `DirectoryTree` component itself.
  - **Command palette entry**: added a purely frontend-synthesized `ActionDescriptor`
    (`id: 'client.toggleDirectoryTree'`, `defaultShortcuts: [{ key: 'F10', alt: true }]`) to
    `actionsWithFavourites()`, following the exact precedent `favouriteActions()` already
    established for non-backend palette entries - confirmed via investigation that neither the
    Settings dialog nor the embedded-terminal toggle had ever been added to the palette this way,
    so this establishes rather than follows an existing "keyboard-only toggle in the palette"
    convention. Dispatch added as an early-return branch in `action-command-controller.ts`'s
    `invokePaletteAction`, alongside the existing `core.favourite.*`/`core.showProperties`
    special cases, via a new `ActionCommandControllerContext.toggleDirectoryTree()`.

  **Verified**: `pnpm exec tsc --noEmit` clean; `biome check .` clean (same 3 pre-existing
  `noDescendingSpecificity` warnings, none in touched files); full `pnpm exec vitest run`:
  1351/1351 passing, zero flakes. Touched test files, each re-run individually and passing in
  full: `tree-keybindings.test.ts` (15/15 — added PageUp/PageDown, Tab/Shift+Tab in both
  directions, and modifier-guard rejection), `directory-tree.test.ts` (14/14 — root has no
  toggle, lazy-expansion moved to a child fixture, PageDown clamping, `onTabOut` direction,
  `registerFocus`), `workspace-layout.test.ts` (20/20 — added `onPaneCycleBoundary` claims
  focus / declines / is not consulted mid-cycle), `app-shell.test.ts` (109/109 — extended the
  existing Alt+F10 test with a focus-on-open assertion via `vi.waitFor` on
  `document.activeElement`, added a new command-palette-toggle end-to-end test),
  `theme.test.ts` (12/12 — updated the `:focus-within`-qualified selector regexes). Manual
  browser verification was attempted again
  (mock runtime) but blocked by the same unrelated dev server already holding the configured
  port in this environment as during the original implementation pass - not safe to resolve by
  killing another session's process. Relied on the DOM-level integration tests above (real
  keyboard dispatch, `document.activeElement` assertions, full `AppShell` mount) instead of
  eyes-on-screen confirmation - flagged here honestly, as before.
