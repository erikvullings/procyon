# 0238 Basket as a virtual folder

Status: done
Priority: high
Subsystem: frontend
Depends on: 0165

## Context
The collection basket (0165) is a bespoke list: checkboxes, a Location column, a bottom
icon bar, and its own selection model. User-reported problems (v0.4.0):

- An item edited after collection (e.g. saved from the SVGO plugin) shows as stale/unavailable
  and never refreshes by itself. The cause: `recheckBasket` verifies with
  `expectedSize`/`expectedModifiedAt`, so any edit fails identity and becomes `stale`.
- The basket can't be reached with Tab, so it can't be driven from the keyboard.
- The copy icon says "Copy 2 items" while 11 are selected: `basketSources` silently drops
  every selected item that isn't `ready` (the stale ones).
- The user wants it to behave like a regular file pane over a virtual folder.

## Acceptance Criteria
- [x] The basket tab renders through the normal pane/`DirectoryTable` pipeline with the regular
  columns (name, size, modified, ...); no Location column.
- [x] Entries are grouped under non-selectable folder header rows showing the parent folder
  (`~/Downloads` for the home folder, otherwise the full path). The cursor, Space/Shift/Insert
  selection, typeahead and Home/End skip the header rows.
- [x] Tab moves focus between the basket pane and the other pane, as for any pane. Arrow keys,
  Space/Insert/Shift selection, select all/none and invert work as in a regular pane. There are
  no checkboxes.
- [x] F3 (view), F4 (edit), F5 (copy to the other pane), F6 (move to the other pane), F8/Shift+F8
  (delete), F2/Shift+F6 rename and multi-rename act on the basket's selection (or cursor), using
  each entry's real location and parent folder.
- [x] The bottom icon bar is removed. A way remains to clear the basket and to remove items from the
  basket without deleting them from disk (e.g. Delete/Backspace or a context-menu entry).
  Collecting via the toolbar/F5 from the other pane still works.
- [x] Items whose files changed after collection are refreshed automatically (new size/modified
  time, status `ready`). They are only `missing`/`moved` when the identity really is gone.
  Refresh happens when the basket opens, before an action runs, and when an operation or watcher
  change touches an item's parent folder.
- [x] Action counts match the selection. Unavailable items are reported, not silently dropped.
- [x] The status bar keeps the file/folder/size summary.
- [x] Mock, HTTP and Tauri adapters all work (frontend-only change; no new backend API needed).

## Implementation Notes
- The model is in `frontend/src/features/basket/basket.ts` and the view in `basket-view.ts`.
  Wiring is in `frontend/src/app/app-shell.ts`: `basketTabIds`, `basketFor`, `updateBasket`,
  `recheckBasket`, `runBasketAction`, `openBasket`, and rendering ~5180.
- Treat the basket like `search://` virtual tabs: entries carry their real `location`. The normal
  table already has a `groupByParentPath` sort and `showFullPath` rendering for search tabs
  (`directory-table.ts`). Add header rows to the table as a separate, opt-in row model.
- Rename (`pane-content-builder.ts` `onRename`) and multi-rename
  (`openMultiRenameForActivePane`) currently assume `active.location` is the parent folder. For
  virtual tabs they must use each entry's parent.

## Agent Notes

- 2026-10-06: Implemented the basket as a frontend-only virtual directory layered over the normal
  pane/table pipeline. Backend workspace tabs keep a real filesystem location so
  `addTransientTab` remains valid; the frontend marks basket tab IDs and supplies a
  `basket://local/<workspace>` `PaneDirectoryView`.
- 2026-10-06: Added opt-in `DirectoryTable` parent-folder group rows. They are display-only rows;
  cursor, selection, scrolling, and typeahead still use entry indices, so Space/Insert/Home/End and
  pane focus behaviour stay shared with normal directories.
- 2026-10-06: Removed the bespoke basket list/check boxes/icon action bar. Backspace removes the
  active basket selection from the basket without touching disk; Delete/F8/Shift+F8 still use the
  normal operation engine for delete/trash. The toolbar keeps Add/Open basket controls and the
  pane status bar keeps the summary.
- 2026-10-06: Basket refresh now verifies stable identity without expected size/mtime, then lists
  each parent folder to refresh size/modified/revision snapshots. It runs on open, before file
  operations, and on operation/watcher-driven pane refreshes. Unavailable selected basket entries
  are reported via the basket unavailable-count message instead of being silently dropped.
- 2026-10-06: Rename and multi-rename resolve each virtual entry's real parent folder; multi-rename
  can apply across multiple parents by deriving the destination per entry.
- 2026-10-06: Verified with `pnpm --dir frontend exec tsc --noEmit -p .`,
  `pnpm exec biome check frontend/src`, affected Vitest coverage (391 tests), and the full frontend
  suite (151 files / 2322 tests). One full-suite run exposed an unrelated/flaky dispatcher failure;
  rerunning the isolated dispatcher file and then the full suite passed.
