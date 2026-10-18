# 0270 Per-folder view memory

Status: done
Priority: low
Subsystem: backend, frontend
Depends on: 0029, 0134

## Context
Photos want grid view, Downloads wants date sort; users re-apply this every visit. Remember view mode, sort and columns per folder (or per folder pattern).

## Acceptance Criteria
- View mode, sort and visible columns are remembered per location and restored on navigation; an indicator and a 'forget for this folder' action exist.
- Stored in settings via the migration machinery (0030); bounded size with LRU pruning; works for remote locations keyed by connection + path.
- Opt-out setting; defaults unchanged for folders never customized.

## Implementation Notes
- Respect browser/Tauri parity: state is backend-owned settings, not localStorage.

## Agent Notes
- Created 2026-10-18 from the media/playlist feature discussion. 
- 2026-10-18 claude: Backend-owned (no localStorage): fm-settings gains Settings.remember_folder_views (opt-out, default on) and Settings.folder_views (FolderViewMemory: sort, columns, hidden, folders-first, view mode, icon size; MRU-first, unique per provider+URI, capped at 500 with oldest pruned on save/load; location URIs sanitised like saved searches). No schema bump needed (struct-level serde default; legacy files load with the feature on). The list is NOT part of SettingsDto: update_settings preserves it so a stale client cannot wipe it. FileManagerService::apply_workspace_command wraps the workspace service: an updateView touching sort/columns/hidden/foldersFirst/viewMode/iconSize records the tab's view for its folder; navigateTab (push/back/forward) restores the destination's remembered view via an internal updateView (only differing fields), identical on Axum and Tauri. Snapshot DTO gained viewRemembered; POST /api/v1/folder-views/forget + Tauri forget_folder_view + client.forgetFolderView. Frontend: view-mode button shows a dot when the folder's view is remembered, menu offers 'Forget the view for this folder'; settings editor switch for the opt-out; navigation reloads in the background when the restored view changes sort/hidden/foldersFirst (the first listing used the previous folder's options). Limits: the indicator reflects the last listing, so a just-customised folder shows it after the next reload; tabs opened directly on a folder (addTab) are not restored until their first navigation; quick-filter-only changes are not remembered. Tests: fm-settings (LRU/forget/persistence/legacy), folder_views unit tests, fm-application/tests/folder_view_memory.rs (restore, opt-out, forget, settings-save keeps memory), pane/navigation tests.
