# 0297 Semantic settings redesign: global folders, workspace multi-select, per-file details

Status: done
Priority: high
Subsystem: frontend + backend
Depends on: none

## Context
Settings → Semantic (RAG) was hard to read, and index management was gated on the *current*
workspace: `Remove from indexing…` only showed when the current workspace referenced the root.
Folders whose workspace was deleted (e.g. ephemeral window workspace `505aa90e`) could not be
removed at all. Found while investigating the 0.7.0-8 empty-index report (startup restore/reconcile
race fixed in `apps/fm-desktop/src-tauri/src/lib.rs`; corpus-theme failure now reports
`corpusThemeIndexNotBuilt`; tenant = workspace id, completion receipts stale after an index swap).

## Acceptance Criteria
1. Removing a folder from indexing never depends on the current workspace.
2. A workspace without any semantic index (no root assigned to it and no all-workspace root) hides
   the corpus-themes toolbar icon and disables its command-palette entry.
3. A root whose referenced workspaces no longer exist becomes available in all workspaces.
4. Each folder has a workspace multi-select; none selected (or all selected) = all workspaces.
5. `Indexing on` is merged with `Pause indexing` into one row (state + toggle).
6. Section title reads `Semantic library: Gemma (incl. images, audio, video)` (only enabled media).
7. Gemma media checkboxes sit on one line.
8. `Folder open in a pane` block removed; a compact include button remains when needed.
9. The completed `Removed folders` list is gone; unfinished cleanups stay visible with Resume.
10. Semantic library and `AI summaries and answers` are collapsible.
11. Included-folder card: path is a link opening the folder in a pane, decoded (`%20` → space);
    `Last worker launch` removed; files behind `Unsupported MIME type` / `Over-size files` listed.
12. The 64 MiB source limit is a user setting (`semanticMaxSourceMib`, 0 = no limit).

## Implementation Notes
- Backend model: `EnrolledRoot::applies_to` — empty `workspace_references` = all workspaces
  (no schema bump; serde default). `detach_workspace` of the last reference therefore yields
  all-workspaces. Enrolling a location that is already all-workspaces does not narrow it.
  `crates/fm-semantic-library/src/policy.rs`, `catalog.rs` (`remove_root_workspace_scopes`).
- Ingestion: roots with no references are fed into every persistent (non-ephemeral) workspace
  tenant; `FileManagerService::refresh_semantic_workspace_directory` (called before every
  reconcile entry point) refreshes that list, applies the settings size limit and prunes
  references to deleted workspaces (skipped when the workspace store is empty).
  `fm-application/src/service.rs`, `semantic_indexing.rs`.
- New operation `updateSemanticRootWorkspaces` (`POST /api/v1/semantic/library/root-workspaces`,
  Tauri `update_semantic_root_workspaces`, mock/http/tauri clients); desktop reconciles the root
  afterwards so gaining workspaces get indexed. New DTO `SemanticSkippedFilesDto` on root status
  (in-memory, last reconciliation, capped at 200 files per reason).
- Size limit: `Settings.semantic_max_source_mib` (default 64). Applies to non-PDF files
  (min with the resource profile). PDFs stay bounded by the worker's advertised cap (≤ 200 MiB) —
  that cap cannot be raised from settings. `worker_feed_plan_with_pdf_limit` gained a
  `max_source_bytes` parameter.
- Frontend: `semantic-library-management.ts` (collapsible, merged ingestion row, workspace
  selector, skipped-file lists, link), `settings-editor.ts` (max file size, collapsible AI
  summaries), `knowledge-wiring.ts`/`actions-wiring.ts`/`app-shell.ts` (themes gating via
  `themesWorkspaceIndexed` + `featureAvailable`). i18n for all 8 languages; dead
  `lastWorkerDevice/metal/cpu/cpuForced` keys removed.

## Stages
- [x] A frontend declutter
- [x] B backend: global removal, all-workspaces model, dangling-workspace pruning
- [x] C workspace multi-select UI + themes gating
- [x] D skipped-file lists + size-limit setting
- [x] E i18n, `api:export`/`api:generate`, tests, lint

## Agent Notes
- All-workspaces is a flag-free "empty = all" model (decided without asking; mirrors the user's
  wording "no selection = available in all"). Indexing duplicates records per workspace tenant;
  embeddings are cached by content key so there is no re-embedding cost.
- Not done / known limits: no per-file "index anyway" override — raise or disable the global limit
  instead; a newly created workspace gets all-workspace roots on the next reconcile (startup,
  settings change, or the 30-minute interval), not instantly; skipped-file lists are in memory and
  refill on the next reconciliation after a restart; themes availability re-checks on workspace
  switch, settings close and startup (not on a folder add made in another window).
- Per-file ineligibility attribution relies on single-candidate feed plans (one entry per call).
- `pnpm api:check` reports a diff only because the working tree is uncommitted; regenerated files
  are included in this change.
