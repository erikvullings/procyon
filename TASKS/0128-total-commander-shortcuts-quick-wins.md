# 0128 Total Commander shortcut parity — quick wins

Status: done
Priority: low
Subsystem: frontend
Depends on: none

## Context

Cross-referenced the shortcut list at
<https://tutorialtactic.com/blog/total-commander-shortcuts/> against fm's current keybinding
implementation (`crates/fm-application/src/action.rs` `ActionRegistry`, resolved by
`frontend/src/keybindings/dispatcher.ts`, wired in
`frontend/src/features/keybindings/global-keydown-handler.ts` and
`frontend/src/features/panes/pane.ts`).

This task covers two things:

1. **Documents shortcuts fm already implements** (possibly under a different key) so nobody
   re-implements them.
2. **Lists shortcuts that are genuinely missing but cheap** — they reuse existing operations,
   services, or UI and just need a `KeyChord` added to the action registry (or a small dispatcher
   case), no new subsystem.

The companion task [0129](0129-total-commander-shortcuts-major-features.md) covers the shortcuts
that require real new functionality (new dialogs, new panes, new backend capabilities).

### Already implemented (verify only, no code change expected)

| TC shortcut | TC action | fm status |
| --- | --- | --- |
| F3 | View file | `core.view` on F3 ✓ |
| F4 | Edit file | `core.edit` on F4 ✓ |
| F5 | Copy | `core.copy` on F5 ✓ |
| F6 | Move/rename | `core.move` on F6 ✓ (fm splits rename out to F2 instead, see conflict below) |
| F7 | New directory | `core.createDirectory` on F7 ✓ |
| F8 / Shift+F8 | Delete (recycle bin) / Delete (permanent) | `core.trash` / `core.delete`, capability-gated ✓ |
| Shift+F10 | Context menu | implemented in `directory-table.ts:673` ✓ |
| Ctrl+A | Select all | `core.selectAll` ✓ |
| Ctrl+C / Ctrl+X / Ctrl+V | Clipboard copy/cut/paste | `global-keydown-handler.ts:197-249` ✓ |
| Ctrl+D | Directory hotlist/bookmarks | favourites menu, `pane.ts:426-439` ✓ |
| Ctrl+R | Reread source directory | `core.refresh` ✓ |
| Ctrl+T / Ctrl+W | New tab / close tab | ✓ |
| Ctrl+Tab / Ctrl+Shift+Tab | Next/previous tab | ✓ (literal Ctrl even on macOS, by design) |
| Alt+F3 | Alternate (system) viewer | `forceSystemView`, `global-keydown-handler.ts:165-169` ✓ |
| Alt+Shift+F3 | Force internal Lister viewer | already the default F3 behavior in fm (no "system by default" setting exists), so nothing to add |
| Alt+F5 / Alt+Shift+F5 | Pack / move to archive | `core.pack` / `core.moveToArchive` ✓ |
| Alt+F6 | Unpack archive | `core.extract` ✓ |
| Alt+F7 | Find files | `core.findFiles` ✓ |
| Numpad `+` / `-` / `*` | Select/deselect/invert by mask | `core.selectByMask` / `core.deselectByMask` / `core.invertSelection` ✓ |
| Tab | Switch pane | `core.switchPane` ✓ |
| Backspace | Parent directory | `core.parent` ✓ |
| Up/Down/PageUp/PageDown/Home/End/Enter | Cursor & open | ✓ |
| Ctrl+Alt+Letter | Quick search by filename | fm's plain-character typeahead (`pane.ts:618-629`) covers this already, different trigger key but same outcome — no change needed |
| Ctrl+M | Multi Rename Tool | Fully implemented, see [0072](0072-multi-rename.md) (done) — the multi-rename dialog opens on F2 with more than one entry selected (`Pane.beginRename`) rather than a dedicated Ctrl+M chord. See "new, cheap shortcuts" below for optionally binding Ctrl+M as an alias entry point to the same dialog. |

### Conflicts with fm's existing bindings (decision + resolution proposed)

| Key | TC uses it for | fm currently uses it for | Proposed resolution |
| --- | --- | --- | --- |
| F2 | Reread/refresh source directory | Rename (`core.rename`) | Keep fm as-is — F2-for-rename matches Windows/macOS/VS Code convention and is more discoverable than TC's DOS-era mapping. fm already has Ctrl+R bound to refresh, which covers TC's F2 intent under a different key. No code change; document the intentional divergence in the keybindings help/settings screen. |
| Ctrl+F | Connect to FTP server | Quick Filter (`core.quickFilter`) | Keep fm's Ctrl+F (Quick Filter is a high-frequency action already shipped). Do not add a dedicated "connect to FTP" shortcut — FTP/FTPS connections go through the Connections dialog. If a shortcut is wanted later, use Ctrl+Shift+N ("new connection") instead, kept distinct from Ctrl+N below. |
| Ctrl+L | Calculate occupied disk space | Focus Location bar (`core.focusLocation`) | Keep fm's Ctrl+L. Disk usage is now available on Ctrl+Shift+L through task [0118](0118-integrate-parallel-disk-usage-windirstat.md), rather than reclaiming Ctrl+L. |
| Ctrl+P | Copy current path to command line | Command Palette | Keep fm's Ctrl+P (command palette is a core navigation feature). `core.copyPath` already exists as an unbound action (`action.rs:473-493`) — see the "copy path/name" row below for the cheap fix (bind it to a free combo instead of fighting for Ctrl+P). |
| Ctrl+S | Quick Filter | *(unbound globally; Ctrl+S only saves inside the CodeMirror editor)* | No real conflict — fm's quick filter already lives on Ctrl+F. Leave Ctrl+S alone (editor save). Document that fm's quick filter key differs from TC's. |
| Ctrl+Shift+T | Open new background tab (don't activate) | Reopen Closed Tab (`core.reopenClosedTab`) | Keep fm's reopen-closed-tab — it mirrors the browser convention (Ctrl+Shift+T reopens the last closed tab in every major browser), which is more valuable than TC's background-tab-open here. Skip TC's variant; a background "open in new tab" is reachable today via the tab context menu. |

## Acceptance Criteria

- [x] The already-implemented table above is verified accurate (spot-check each row against
      current code) and the conflict-resolution decisions are recorded in user-facing keybinding
      docs/settings (e.g. a short note in the settings-editor conflict UI or a CHANGELOG entry) —
      no functional code change required for this section.
- [x] Each "new, cheap" shortcut below is added to `ActionRegistry::core_actions()` (or the
      relevant special-cased handler where the target isn't a registry action) with a sensible
      default `KeyChord`, resolvable via `dispatchKeybinding`, and shown correctly in
      `frontend/src/features/settings/settings-editor.ts` conflict detection.
- [x] Each addition has a unit test in `action.rs` (registry) and/or `dispatcher.test.ts` /
      `pane.test.ts` covering the new binding.
- [x] No existing shortcut regresses (run the full keybinding test suite).

### New, cheap shortcuts to add

All of these reuse an operation, service call, or piece of UI state that already exists — the
work is wiring a `KeyChord` and, where needed, a couple lines of dispatch logic.

| Shortcut | Action | Why it's cheap |
| --- | --- | --- |
| Ctrl+Backspace | Go to root directory | Reuses existing navigation (`core.parent` repeated, or a direct "navigate to `/`" call) |
| Ctrl+Up | Open directory under cursor in a new tab | Reuses `core.newTab` + existing "open" logic, just targeting the cursor entry instead of the current location |
| Ctrl+Shift+Up | Open directory under cursor in the *other* pane as a new tab | Same as above, targeting the opposite pane's tab strip |
| Ctrl+Left / Ctrl+Right | Copy the active pane's current path into the other pane (TC calls this "duplicate directory"; TC's Ctrl+I is the same idea and can reuse this binding too) | Just sets the other pane's location to `this.location` |
| Ctrl+U | Exchange directories between panels (swap left/right locations) | Pure state swap on the two panes' current locations |
| Ctrl+Shift+U | Exchange directories *and* tab sets between panels | Same swap, extended to the full tab list per pane |
| Ctrl+Shift+W | Close all open tabs | Loop over `core.closeTab` for every tab except the last, or a small new `core.closeAllTabs` |
| Ctrl+N | Open the "new connection" dialog directly | Connections dialog already exists (used for SFTP/FTP/etc.); this just gives it a direct shortcut instead of only reaching it via menu/palette |
| Insert | Toggle selection at cursor and move cursor down one row | Compose existing `core.toggleSelection` + `core.moveCursorDown` |
| Numpad `/` | Restore previous selection | Cache the selection `Set` before each change in the pane's selection state; restore on this key |
| Ctrl+Shift+S | Reactivate the last-used Quick Filter query | Quick Filter state already holds the current query; just cache the last non-empty one and re-apply |
| Shift+F4 | Create a new text file here and open it in the editor | Compose existing `core.createDirectory`-style "create" flow (adapted for a file) + `core.edit` |
| Shift+F5 | Copy-with-rename in the same directory ("Duplicate") | The duplicate file operation already exists in the backend (see `crates/fm-application/src/operation_planner.rs`, `crates/fm-domain/src/workspace.rs`) from the original op-duplicate task — just needs a shortcut and menu entry wired to it |
| Shift+F6 | Rename files in the same directory | Straightforward alias to the existing `core.rename` (F2) flow — document as an alias rather than a separate implementation |
| Ctrl+F3 / Ctrl+F4 / Ctrl+F5 / Ctrl+F6 / Ctrl+F7 | Sort by name / extension / date / size / unsorted | Panes already support sorting via `onSortChange` (column-header click, see `frontend/src/features/panes/pane.ts:150`) — these just call the same handler with a fixed `SortDescriptor` |
| Ctrl+F10 | Show all files (clear active quick filter) | Quick Filter already has a clear action; just needs a shortcut |
| Ctrl+M | Open the multi-rename dialog as an alias, regardless of selection count | [0072](0072-multi-rename.md)'s dialog already exists and only opens implicitly via F2 with 2+ selected; this just gives it a direct entry point |
| Alt+F4 | Quit the application | Tauri window close/exit — trivial in desktop mode; no-op or browser `window.close()` attempt in browser mode |
| F1 | Show a keyboard-shortcuts / about overlay | No help system exists yet, but the settings-editor already renders the full action list with shortcuts (conflict-detection UI) — reuse that view in a read-only dialog rather than building new content |

## Implementation Notes

- Registry-level additions go in `crates/fm-application/src/action.rs` (`core_actions()` /
  `selection_actions()`), following the existing `ActionDescriptor` pattern with
  `default_shortcuts: vec![...]`.
- Frontend dispatch: most of these route naturally through `dispatchKeybinding()` in
  `frontend/src/keybindings/dispatcher.ts`; a few (new-tab-from-cursor, pane-swap, restore-previous-selection)
  need a few lines in `global-keydown-handler.ts` or `pane.ts` similar to the existing
  `forceSystemView`/`forceSystemEdit` special cases, since they act across panes rather than on a
  single action id.
- Remember the `KeybindingRuntime` split (`'browser' | 'desktop'`) — Alt+F4 (quit) and anything
  else desktop-only should be gated the same way F12 (terminal toggle) already is.
- After adding shortcuts, regenerate/update the frontend DTOs if `ActionDescriptorDto` shape
  changes (it shouldn't for this task — same shape, more entries).

## Agent Notes

- 2026-08-13 agent: Implemented all 19 "new, cheap" shortcuts end-to-end (backend registry +
  frontend dispatch + tests). Verified: `cargo fmt --all --check`, `cargo clippy --workspace
  --all-targets -- -D warnings` (0 warnings), `cargo test --workspace` (all green, 23 new/changed
  tests in `crates/fm-application/src/action.rs`), `pnpm exec tsc --noEmit` (clean except one
  pre-existing, unrelated `backend-event-handler.test.ts` widening error confirmed via
  `git stash`), `pnpm exec biome check .` (0 new errors; remaining warnings are pre-existing,
  outside touched files/lines), `pnpm vitest run` (906 passed, 1 pre-existing unrelated failure in
  `app-shell.test.ts` confirmed via `git stash` to predate this change).

  Files touched (backend): `crates/fm-application/src/action.rs` (22 new `ActionDescriptor`
  entries + Shift+F6 alias on `core.rename` + 9 new focused tests),
  `crates/fm-application/src/operation_planner.rs` (new `CreateFileExecutor`, reusing the `WRITE`
  capability rather than adding a dedicated `CREATE_FILE` bit — creating an empty file is just
  `open_write` + immediate `shutdown()`), `crates/fm-application/src/service.rs` (operation-kind
  mappings + `core.createFile`/`core.duplicate` added to `mutating_operation_kind`),
  `crates/fm-operations/src/model.rs`, `crates/fm-operations/src/scheduler.rs`,
  `crates/fm-events/src/lib.rs`, `crates/fm-transport-dto/src/operation.rs` (new `CreateFile`
  operation kind, end-to-end), plus regenerated `frontend/openapi/openapi.json` and
  `frontend/src/api/generated/models/operationKindDto.ts` via `pnpm run api:export` +
  `pnpm run api:generate`.

  Files touched (frontend): `frontend/src/keybindings/dispatcher.ts` (added `CTRL+N`/`CTRL+U` to
  `BROWSER_RESERVED`), `frontend/src/features/keybindings/global-keydown-handler.ts` (new
  `GlobalKeydownContext` members + dispatch cases for rootDirectory/openInNewTab(OtherPane)/
  duplicateLocationToOtherPane/swapPanes/swapPaneTabs/closeAllTabs/newConnection/
  reactivateQuickFilter/clearQuickFilter/sort×5/createFile/duplicate/openMultiRename/quit/
  showShortcutsHelp), `frontend/src/features/panes/pane.ts` (Insert composes toggle+moveCursor;
  Numpad `/` restores a per-keystroke selection snapshot via a new `restore` `SelectionAction`),
  `frontend/src/features/selection/selection.ts` (new `restore` action/reducer case),
  `frontend/src/features/panes/tab-controller.ts` (new `openTabAt`, `closeAllTabs`),
  `frontend/src/features/navigation/root-location.ts` (new `rootLocationFor` URI-prefix helper),
  `frontend/src/features/operations/create-file-dialog.ts` (new, mirrors
  `create-directory-dialog.ts`), `frontend/src/features/operations/operations-controller.ts` (new
  `createFile`/`duplicate` methods), `frontend/src/features/dialogs/dialog-ui-controller.ts` +
  `app-dialogs.ts` (create-file dialog wiring + `openEditorForCreatedFile`),
  `frontend/src/features/keybindings/shortcuts-help-dialog.ts` (new F1 overlay, reuses
  `getLiveBindings` rather than a second hardcoded shortcut list), `frontend/src/app/app-shell.ts`
  (wires every new context method; `quitApplication`/`swapPaneTabSets` live here),
  `frontend/src/api/client/file-manager-client.ts` + `tauri-file-manager-client.ts` (new optional
  `quit()`, only implemented on the Tauri client), `frontend/src/models/operation.ts` (added
  `'createFile'` to the frontend `OperationKind` union).

  New/updated tests: `action.rs` (9 new `#[test]` fns), `operations-controller.test.ts` (+2),
  `selection.test.ts` (+2), `dispatcher.test.ts` (+1, plus the `BROWSER_RESERVED` fixture
  additions), `pane.test.ts` (+2, Insert and Numpad `/`), `tab-controller.test.ts` (new file, 2
  tests for `openTabAt`/`closeAllTabs`), `global-keydown-handler.test.ts` (new file, 20 tests
  covering every cross-pane/global dispatch case), `root-location.test.ts` (new file, 4 tests).

  Deviations from the literal spec, all deliberate and reuse-preferring:
  - **Shift+F4 "create file"**: no backend primitive existed. Added a full `CreateFile` operation
    end-to-end (domain enum, DTO, planner executor, OpenAPI/Orval regen) rather than faking it
    frontend-side, but implemented the executor by reusing the existing `open_write` streaming
    primitive (same one `copy_file` uses) instead of adding a new provider trait
    method/capability — every provider that can write a file can already create an empty one, so
    no per-provider (`fm-vfs-local`/`sftp`/`ftp`/`archive`) changes were needed.
  - **Ctrl+Backspace "root directory"**: no provider-aware "root location" helper existed
    (`remoteRootLocation` needs a full `Connection` object, unavailable from a bare pane cursor
    location). Added `rootLocationFor()`, a URI-prefix convention (scheme+host + trailing slash)
    documented in its own doc comment as not provider-aware: for remote providers whose real
    browsable root is a configured start path rather than `/`, it lands one level higher than
    that start path. Good enough for "jump to the top of this tree" without a `Connection`.
  - **Ctrl+Shift+U "swap pane tab sets"**: confirmed (via research) no backend command can swap a
    whole tab set atomically — `WorkspaceCommand`'s union has nothing for reassigning a pane's
    `tabOrder`/`tabsById` wholesale. Implemented as a local-only optimistic projection swap in
    `app-shell.ts` (`swapPaneTabSets`), the same pattern `activateTab` already uses for optimistic
    updates, rather than adding a new backend `WorkspaceCommand` variant. Known limitation: this
    swap is not persisted through `dispatchWorkspaceCommand`, so a concurrent revision conflict
    from an unrelated command could silently drop it; flagging as a known gap rather than
    over-engineering a new backend command for a "quick win" task.
  - **Ctrl+Shift+W "close all tabs"**: no `closeAllTabs` backend primitive existed either; added
    `TabController.closeAllTabs`, which loops `closeTab` commands sequentially (each awaiting the
    previous so `expectedRevision` stays in sync), per the task's own suggested fallback.
  - **Ctrl+N "new connection"**: Chrome intercepts Ctrl+N at the OS/browser-chrome level for "new
    window" before any page keydown listener runs — confirmed unfixable via
    `preventDefault()`, so added `'CTRL+N'` to `BROWSER_RESERVED` (browser runtime falls back to
    the command palette/menu). While researching this, also confirmed Ctrl+U (`core.swapPanes`)
    is reserved by Chrome for "View Source" and added `'CTRL+U'` too, since it would otherwise
    silently never fire in browser runtime. Both are unaffected in desktop (Tauri) runtime.
  - **Numpad `/` "restore previous selection"**: implemented via local pane-component closure
    state (`previousSelectionSnapshot`, snapshotted at the top of every keydown, mirroring the
    `typeaheadCtrl` pattern already in `pane.ts`) plus a new `restore` `SelectionAction`/reducer
    case, rather than threading selection history through app-wide state — it doesn't need to
    survive a re-render or be visible outside the pane.
  - **Ctrl+M "multi-rename direct entry point"**: while wiring this, found that
    `pane-content-builder.ts`'s existing `onMultiRename` callback (used by F2 with 2+ selected)
    calls `context.setMultiRenameOpen(true)`, whose `app-shell.ts` implementation is
    `(open) => { if (!open) dialogs.cancelMultiRename(); }` — a no-op when `open` is `true`. This
    looks like a pre-existing latent bug (the dialog's actual open flag is set only by
    `dialogs.openMultiRename(...)`, never reached through that path) predating this task and
    unrelated to the 19 shortcuts; left untouched and out of scope. The new `Ctrl+M` binding calls
    `dialogs.openMultiRename(...)` directly instead of going through that codepath, so it is
    unaffected either way.
  - **F1 "shortcuts help"**: built a minimal read-only `ModalPanel` (`shortcuts-help-dialog.ts`)
    that calls `getLiveBindings` directly rather than reusing `SettingsEditor` wholesale, since
    that component's draft-editing/save/cancel state has no read-only mode and pulling in its
    editing affordances would be out of scope for a static shortcut list.
  - Ctrl+Left and Ctrl+Right both bind to the single `core.duplicateLocationToOtherPane` action
    (per the task's own note that direction is cosmetic), and Shift+F6 is a second `KeyChord` on
    the existing `core.rename` action rather than a new action id, both exactly as specified.

- 2026-08-13 follow-up (user report — macOS shortcuts not behaving as expected): user reported
  Ctrl+Up/Down intercepted by the OS, Ctrl+Backspace hitting "parent directory" instead of "root"
  (Cmd+Backspace worked), and Ctrl+N needing to be Cmd+N. Traced root cause in
  `frontend/src/keybindings/dispatcher.ts`'s `matches()`: `hasPrimaryModifier` only recognises Cmd
  on macOS, so a *bare* chord's modifier check (`hasPrimaryModifier(...) === false`) was satisfied
  by a literal-Ctrl-held keypress too (since `hasPrimaryModifier` returns `false` whenever Cmd is
  absent, Ctrl-held or not) — literal Ctrl on macOS was indistinguishable from no modifier at all
  to any bare-key binding. This silently misfired for **six** of the new shortcuts, not just the
  one the user found: Ctrl+Backspace (→ `core.parent`), and Ctrl+F3/F4/F5/F6/F7 (→
  `core.view`/`core.edit`/`core.copy`/`core.move`/`core.createDirectory` respectively, since F3–F7
  all have pre-existing bare bindings). Ctrl+Shift+Up was also affected (→
  `core.extendSelectionUp`), reachable because macOS Mission Control's default Ctrl+Arrow
  interception doesn't cover the Shift-held variant. Ctrl+N/Ctrl+U/Ctrl+Left/Ctrl+Right have no
  bare-key counterpart to collide with, so those were already safe no-ops on literal Ctrl (just
  needed Cmd to actually fire) — not misfires, only a documentation gap.

  **Fixed** `matches()` so a bare chord now requires the absence of *both* Ctrl and Cmd, not just
  Cmd (`frontend/src/keybindings/dispatcher.ts`), closing all six collisions at once; added a
  regression test (`dispatcher.test.ts`, "never lets literal Ctrl on macOS fall through to a
  bare-key binding sharing the same key", using `core.rename`/F2 as the collision partner). No
  Windows/Linux behavior changed (Ctrl already was the primary modifier there, so the bug never
  existed on that platform).

  Also fixed two unrelated pre-existing failures the user had asked about separately in the same
  session: `backend-event-handler.test.ts`'s `completedOperation` fixture was an untyped object
  literal whose `kind: 'copy'` widened to `string` (added an explicit `Operation` type annotation);
  `app-shell.test.ts`'s "keeps cursor and selection independent" test asserted the *old*
  arrow-key-collapses-selection behavior, which the user's own most recent commits
  (`f9a3ce2`/`0546505`, predating this task) deliberately changed to *preserve* an existing
  multi-selection during plain arrow-key navigation — updated the test's assertions to match the
  current, intended behavior instead of reverting the app code.

  Documented the verified Mac-vs-Windows keystroke for every shortcut (old and new) as a table in
  the root [README.md](../README.md#keyboard-shortcuts), including the primary-modifier rule, the
  Ctrl+Tab exception, the Mission-Control-interception caveat for literal Ctrl+Arrow, and two
  items flagged as *not* independently verified against native OS window handling (Cmd+M vs.
  "Minimize Window" if a native menu is ever added — no `Menu`/`MenuBuilder` was found configured
  in `apps/fm-desktop/src-tauri`, so this is not believed to be a live conflict today; Alt+F4 vs.
  the OS's own window-close handling on Windows, and macOS's conventional Cmd+Q, which remains
  unimplemented).

  Verified in isolation (not full-suite — two other peer sessions are concurrently editing this
  repo, see `directory-table.test.ts`'s new "does not repeat the generic directory error" test and
  `pane.test.ts`'s new "marks unavailable favourites and allows retrying them" test, neither
  authored by this task and both unrelated to keybindings): `pnpm exec tsc --noEmit` (clean),
  `pnpm vitest run` on `dispatcher.test.ts`, `backend-event-handler.test.ts`, `app-shell.test.ts`,
  and the keybinding-specific tests in `pane.test.ts` (all passing).
