# 0129 Total Commander shortcut parity — features requiring new subsystems

Status: open
Priority: low
Subsystem: frontend
Depends on: none

## Context

Companion to [0128](0128-total-commander-shortcuts-quick-wins.md). That task covers shortcuts fm
already has (or can add cheaply by reusing existing operations/UI). This task covers shortcuts
from https://tutorialtactic.com/blog/total-commander-shortcuts/ whose Total Commander behavior
requires a genuinely new piece of functionality — a new dialog, a new pane/view mode, new backend
capability, or a platform integration (system tray, etc.) fm does not have today.

Each row below is independently schedulable; split into its own task file when picked up, using
`Depends on: 0129` if the breakdown is kept as a parent/subtask structure, or just reference this
file's context if promoted directly.

**2026-08-14 re-triage:** re-checked every remaining row against the current codebase (a lot has
shipped since the initial pass — 0075, 0126, 0128 all landed, and new tasks 0133/0134 were created
in a separate feature-gap review). Findings below; the candidate table now only lists rows that are
still genuinely open. One row (Alt+F1/Alt+F2) was cheap enough to implement immediately during this
review — see Agent Notes.

## Resolved this review (2026-08-14)

- **Alt+F1 / Alt+F2 (switch panel to a different drive)** — **implemented**. fm already had a
  per-favourite quick-jump action (`core.favourite.${index}`, dispatched in
  `frontend/src/features/keybindings/global-keydown-handler.ts`) wired to the favourites menu
  (0070), but it shipped with `defaultShortcuts: []` — dispatchable and user-rebindable, but nothing
  bound out of the box. Added default `Ctrl/Cmd+1`..`Ctrl/Cmd+9` bindings for the first nine
  favourites in `frontend/src/app/app-shell.ts` (`favouriteActions()`). This is fm's natural
  equivalent of TC's drive-switch shortcuts — jump straight to a saved location instead of a raw
  drive letter, since fm has no drive concept. Original row is no longer accurate ("needs UX design
  before implementation, not just a shortcut binding") — the UX already existed, it was one line.
- **Ctrl+I (sync current panel's path to the other panel)** — **already covered**, confirmed. 0128
  added `core.duplicateLocationToOtherPane` bound to `Ctrl+Left`/`Ctrl+Right`
  (`crates/fm-application/src/action.rs`), which is exactly TC's Ctrl+I behavior. No separate
  binding needed; a literal `Ctrl+I` alias to the same action would be a trivial addition if wanted,
  but isn't necessary for parity.
- **Shift+F2 (compare file lists)** — **already covered**, this row was stale. The "Corrections from
  initial triage" note below already said this was tracked as 0075 (directory comparison and
  synchronization, done) and shouldn't be duplicated here, but an old table row survived anyway.
  Removed.
- **Alt+F8 (command-line history dropdown)** — **declined, superseded**. 0126 (embedded terminal
  drawer, done) ships a real PTY-backed terminal per location with full native shell history —
  strictly more capable than a TC-style command-line-only history dropdown. No separate feature
  needed.
- **Ctrl+Shift+F1 (thumbnails view)** and the **Ctrl+F1/Ctrl+F2 (brief/full view mode)** cluster —
  **merged into [0134](0134-thumbnails-and-grid-view.md)**, a new task created in a broader
  feature-gap review that covers thumbnail generation/caching and a grid/icon view mode. 0134's
  Implementation Notes explicitly flag that a general view-mode switch (not just grid/icon) is worth
  building as the foundation, so brief/full-detail modes remain buildable later on the same
  architecture even though 0134 only ships grid/icon view itself.
- **F9 / bare F10 (pull-down menu bar)** — **merged into [0133](0133-native-menu-bar-content.md)**.
  The original row asked "does fm want a menu bar, or is this TC convention obsolete?" as an open
  design question. Answer: yes, but as the OS-native menu bar (macOS/Windows), not an in-app
  pull-down replica — 0133 populates the currently-empty `install_native_menu` hook (0058/0059/0131)
  with real File/Edit/View/Go/Window/Help content. That supersedes the TC-style in-app pull-down
  menu; no separate in-app widget is planned.
- **Alt+F10 / Ctrl+F8 (directory tree)** and **Alt+Enter (Properties dialog)** — **split into their
  own tasks**, [0139](0139-directory-tree-sidebar.md) and [0140](0140-properties-dialog.md), per
  product decision on 2026-08-14: these two were judged the most plausibly worth building of the
  remaining rows (meaningfully sized, commonly expected in a "state-of-the-art" file manager).
  Removed from the candidate table below; see those files for scope.

## Candidate features still open

| TC shortcut(s) | TC behavior | What's missing in fm | Notes |
|---|---|---|---|
| Ctrl+Q | Quick View panel (live inline preview) | A preview pane rendered alongside the file list, updating as the cursor moves | **Declined by prior product direction**, not just "missing": [0071](0071-file-preview-architecture.md)'s Agent Notes record that cursor-driven automatic preview loading was explicitly reversed on 2026-08-04 (it fetched bytes for every entry the cursor passed over). Preview is intentionally opt-in via F3 only (0088). Re-adding a TC-style auto-follow Quick View would need that product decision revisited first — don't implement this row without re-confirming that. (0071 has since gained a related but distinct feature: pressing the preview key on a *directory* shows its recursive size, TC's other Space-bar behavior — see 0071.) |
| Shift+F1 | Custom columns view menu | fm's directory table has a fixed column set; no per-view column picker | **Corrected 2026-08-19**: more prior art than previously stated. `frontend/src/features/settings/settings-editor.ts` already has a real show/hide checkbox list for 5 columns (`AVAILABLE_DEFAULT_COLUMNS`/`toggleColumn`), persisted in `Settings.defaultColumns`, plus separately persisted drag-resizable column widths (`directory-table.ts`). Still missing vs. TC: it's global (not per-pane/per-view), there's no `Shift+F1` shortcut/quick-menu entry point to it, and the column set itself is fixed (no plugin/custom columns). Remaining gap is smaller than originally scoped — mostly a keybinding + maybe per-pane override, not a new subsystem. |
| Shift+F3 | List only the file under cursor when multiple files are selected | The Lister viewer (F3) always targets the cursor entry; TC's nuance is about selection vs cursor interaction when multiple are selected | Confirmed still missing 2026-08-19: `resolveViewTarget` in `frontend/src/features/keybindings/global-keydown-handler.ts` always resolves F3's target from `selection?.cursorEntryId` only; no `Shift+F3` handling exists. Small viewer-behavior change, but grouped here because it's viewer-internals work, not a pure keybinding addition. |
| Shift+Ctrl+F5 | Create shortcuts/symlinks of selected files | No shortcut/symlink-creation operation exists | Confirmed still missing 2026-08-19 (existing `symlink` handling in `operation_planner.rs` only covers copy-time follow-vs-copy-link policy, not creating new links; no `core.createSymlink`/`core.createShortcut` action exists). Platform-asymmetric: Windows `.lnk` creation is nontrivial; POSIX symlinks are simpler. Needs a new operation type in the operation planner plus per-platform backend support. |
| Ctrl+Z (file-list context) | Edit a per-file "comment" (TC's `descript.ion` sidecar file convention) | fm has no file-comment/metadata-sidecar feature | **Corrected 2026-08-19**: partially covered. [0136](0136-extended-attributes-and-finder-tags.md) (done, 2026-08-17) shipped a real editable per-entry Spotlight comment (`core.editSpotlightComment`, `frontend/src/features/entry-metadata/spotlight-comment-dialog.ts`), reachable from context menu/command palette — but it's macOS-only (`kMDItemFinderComment` xattr; Windows/Linux report the capability `false`), not TC's cross-platform sidecar-file convention. [0145](0145-finder-tags-in-properties-dialog.md) (open) already tracks surfacing it in the Properties dialog too. No `Ctrl+Z` shortcut binds to it. |
| Ctrl+Shift+F2 | "Comments" column view | Depends on the Ctrl+Z file-comment feature above | Confirmed still missing 2026-08-19: no "Comments" column in `AVAILABLE_DEFAULT_COLUMNS` or `directory-table.ts` — Spotlight comments (row above) are dialog-only today. Low priority; only meaningful once file comments are more than macOS-only. |
| Ctrl+F11 | Filter to show only executables | Cross-platform "executable" isn't well-defined (macOS `.app` bundles are directories, Linux relies on the exec bit, Windows on `.exe`) | Reconfirmed still missing 2026-08-19. Needs a platform-aware predicate; low value, evaluate before building. |
| Ctrl+F12 | User-defined, savable filter presets | fm's Quick Filter (Ctrl+F) is ad hoc/session-only, no saved presets | Reconfirmed still missing 2026-08-19 (`frontend/src/features/quick-filter/quick-filter.ts` unchanged in shape — plain-text match, no persistence). Needs a small persistence layer (named filter presets in settings) plus a management UI. |
| Ctrl+F9 | Print the file under cursor | No print integration | Reconfirmed still missing 2026-08-19. Low value for a modern file manager; consider explicitly declining rather than implementing. |
| Shift+Esc | Minimize the app to the system tray | fm has no system-tray integration | Reconfirmed still missing 2026-08-19 (no tray code or config in `apps/fm-desktop/src-tauri`). Needs Tauri tray-icon setup (icon, context menu, restore-on-click) — a genuine new platform integration, desktop-only. |
| Ctrl+Shift+F / Ctrl+Shift+M | Disconnect from FTP / toggle FTP transfer mode (ASCII vs binary) | FTP is a VFS provider with no dedicated connect/disconnect or transfer-mode actions bound to keys | Reconfirmed still missing 2026-08-19 (no ASCII/binary transfer-mode or explicit-disconnect code in `crates/fm-vfs-ftp`). Binary vs ASCII transfer mode is a legacy FTP concept fm's VFS abstraction doesn't currently model; would need provider-level support before any shortcut makes sense. |

## Acceptance Criteria

This is a tracking/scoping task, not an implementation task — "done" means each row has been
triaged into one of: (a) split into its own numbered task with `Depends on: 0129` noted where
relevant, (b) explicitly declined with a one-line reason recorded in this file's Agent Notes
(e.g. "Ctrl+F9 print — declined, low value"), or (c) merged into an existing task (e.g. the
thumbnails-view work folded into 0134). Several rows above are already effectively triaged (b) via
their "Notes" column; what remains is a product decision on the rest (see Agent Notes) — freeze
some in the new Freezer section of `TASKS/README.md`, split others into real tasks, or leave open.

## Implementation Notes

- The "FTP session" cluster (Ctrl+Shift+F/Ctrl+Shift+M) and the "file comments" cluster (Ctrl+Z/
  Ctrl+Shift+F2) still cluster naturally if picked up.
- Check [0134](0134-thumbnails-and-grid-view.md) before scoping the Alt+F10/Ctrl+F8 tree-view work —
  it's a separate UI surface (grid/icon view vs a tree sidebar), but both touch the pane's
  view-mode/layout question and should stay aware of each other.

## Agent Notes

- 2026-08-14: Full re-triage against the current codebase (see "Resolved this review" above).
  Implemented the Alt+F1/Alt+F2 equivalent immediately (default `Ctrl/Cmd+1..9` shortcuts on
  existing `core.favourite.N` actions, `frontend/src/app/app-shell.ts`) since it required no new
  architecture. Confirmed by grep that these are still genuinely missing, not just undiscovered:
  tree/sidebar view, properties dialog, column-picker UI, symlink-creation operation, saved
  filter presets, system tray integration, FTP ASCII/binary transfer mode. Remaining open rows are
  a product-priority discussion, not further investigation — see conversation for the proposed
  split between "split into its own task" vs "freezer."
- 2026-08-14 (follow-up): product decision on the remaining rows — split the directory tree
  (Alt+F10/Ctrl+F8) and Properties dialog (Alt+Enter) rows into their own tasks, 0139 and 0140, as
  the two judged most worth building. The rest (Shift+F1 columns, Shift+F3 viewer nuance,
  Shift+Ctrl+F5 symlink creation, Ctrl+Z file comments, Ctrl+Shift+F2 comments column, Ctrl+F11
  executable filter, Ctrl+F12 saved filter presets, Ctrl+F9 print, Shift+Esc system tray,
  Ctrl+Shift+F/M FTP transfer mode) remain open in this task pending further product input on
  which to split out vs. freeze.
- 2026-08-19 Claude: Re-triaged all 10 remaining candidate rows against the current codebase (user
  suspected work since 2026-08-14 had closed some gaps — checked directly rather than trusting the
  table). Two rows had genuinely moved and are corrected above: **Shift+F1** (column picker) already
  has more prior art than stated — a global column show/hide UI and persisted resizable widths exist
  today (`settings-editor.ts`, `directory-table.ts`); the remaining gap is a `Shift+F1` entry point
  and per-pane scoping, not a new subsystem. **Ctrl+Z** (file comments) is now partially covered by
  [0136](0136-extended-attributes-and-finder-tags.md)'s Spotlight comment editor (macOS-only), with
  [0145](0145-finder-tags-in-properties-dialog.md) already tracking the Properties-dialog follow-up.
  The other 8 rows (Shift+F3, Shift+Ctrl+F5, Ctrl+Shift+F2, Ctrl+F11, Ctrl+F12, Ctrl+F9, Shift+Esc,
  Ctrl+Shift+F/M) were independently reconfirmed still fully missing by reading current code
  (`global-keydown-handler.ts`, `operation_planner.rs`, `quick-filter.ts`, `apps/fm-desktop/src-tauri`,
  `crates/fm-vfs-ftp`) — no boxes to check off there. No new numbered task file covers any of the 8.
  Net: this task's own "done" bar (every row triaged into split/decline/merge) is unchanged — nothing
  here newly reaches (a)/(b)/(c) triage, two notes just got more accurate. Still `open` pending the
  same product-priority call on which of the remaining rows to split out vs. freeze.
