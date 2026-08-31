# 0084 Workspace management UI

Status: done
Priority: medium
Owner: unassigned
Agent: unassigned
Area: frontend
Depends on: 0069, 0082

## Context
Tasks 0078–0082 provide persisted workspaces, lifecycle commands, transport parity, events and a
normalized frontend projection. The shell still lacks a user-facing way to list, create, switch,
rename or delete named workspaces. Task 0069 must land first so workspace switching and management
operate on the final tab-per-pane experience.

## Acceptance Criteria
- The application shell exposes a keyboard-accessible workspace switcher showing the active
  workspace and all persisted workspace summaries.
- Users can create, open, rename and delete workspaces through the semantic `FileManagerClient`
  workspace operations from 0082; the frontend never replaces arbitrary workspace JSON.
- Creating a workspace starts from the global defaults owned by 0030, while opening a workspace
  restores its persisted pane/tab/view configuration.
- Switching flushes pending debounced workspace updates, releases obsolete directory subscriptions,
  opens active tabs first, and does not cancel running file operations.
- Destructive deletion requires confirmation, cannot strand the application without a valid active
  workspace, and reports backend/revision conflicts without silent data loss.
- The manager reflects workspace create/rename/open/delete events and remains consistent when the
  same backend is used by another session.
- Vitest tests cover list/switch, create, rename, confirmed deletion, running-operation continuity,
  event refresh, and stale-revision/error behavior.

## Implementation Notes
- Build on 0082's normalized projection and command dispatch; do not introduce a second workspace
  store in component state or browser local storage.
- Task 0069 owns tab rendering and lifecycle. This task coordinates switching/managing whole named
  workspaces and should reuse, not duplicate, its tab behavior.
- Keep directory snapshots separate from persisted workspace definitions and never serialize them
  into workspace state.

## Agent Notes
- 2026-07-31 codex: Created after 0078–0082 delivered the workspace foundation without management
  UI. Best implementation point is directly after 0069; 0082 is already done, and waiting for 0069
  avoids rebuilding the switcher around a provisional one-tab-per-pane shell.
- 2026-08-03 Claude Sonnet 5 (Copilot): Implemented the workspace switcher end-to-end. New
  `frontend/src/features/workspace/{workspace-manager,workspace-switcher,delete-workspace-dialog}.ts`
  (+ `.test.ts`) hold pure sort/recovery helpers and the switcher UI (list, active marker, create,
  inline rename form, delete with a confirmation dialog mirroring `close-last-tab-dialog.ts`).
  `dispatch-workspace-command.ts` now exports `isWorkspaceRevisionConflict` for reuse outside
  command dispatch. `workspace-layout.ts` gained a `registerFlush` attr so `app-shell.ts` can force-
  persist a pending debounced layout edit before switching workspaces. `app-shell.ts` adds
  `activateWorkspace`/`switchWorkspace`/`createWorkspaceAction`/`renameWorkspaceAction`/
  `deleteWorkspaceAction`/`recoverActiveWorkspace`, all built only on `FileManagerClient` semantic
  calls (`createWorkspace`/`openWorkspace`/`renameWorkspace`/`deleteWorkspace`/`listWorkspaces`), a
  toolbar `details.fm-workspace-disclosure` switcher, and `handleBackendEvent` refreshes summaries
  on `workspace.created`/`renamed`/`deleted` and recovers the active workspace if it is deleted
  remotely. Switching flushes the pending layout update, releases the outgoing workspace's per-tab
  caches (aborting subscriptions), loads the active pane's tabs first, and never touches
  `operations`. Deletion always confirms via the dialog and falls back to another summary or a
  freshly created default workspace so the app is never left without an active one; revision
  conflicts surface an actionable message via `isWorkspaceRevisionConflict` instead of silently
  discarding the edit. No second workspace store was introduced (no localStorage, no duplicate
  component state) and 0069's tab-strip rendering is reused unchanged.
  Tests: added `workspace-manager.test.ts` (8), `delete-workspace-dialog.test.ts` (2),
  `workspace-switcher.test.ts` (11), `workspace-layout.test.ts` (+2 for `registerFlush`), and an
  `app-shell.test.ts` "workspace management (task 0084)" describe block (8) covering list/switch,
  create, rename, confirmed deletion with stranding recovery, running-operation continuity across a
  switch, cross-session event refresh (create/rename/delete), and both rename- and delete-revision-
  conflict surfacing. Full frontend suite: 398/398 passing (up from a 369 baseline), `tsc --noEmit`
  clean, Biome clean on all touched files.
