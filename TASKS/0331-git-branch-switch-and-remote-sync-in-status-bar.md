# 0331 Git branch switch and pull/push in the status bar

Status: open
Priority: medium
Subsystem: backend, frontend
Depends on: 0135, 0025

## Context
Git status column/badges shipped in 0135 (`fm-application` uses `git2`; 0151 fixed Windows path canonicalization). Files' Git integration also offers, from the status bar: current branch, branch switch, and pull/push. Procyon's status bar (0025) is not confirmed to show branch or offer these actions. Requested by the user as a gap to close.

Unverified: whether 0135 already exposes branch name or remote state to the frontend. Check before designing.

## Acceptance Criteria
- Status bar shows the current branch and ahead/behind counts when the active directory is inside a repository; hidden otherwise.
- Clicking the branch lists local branches and switches to one; switch refuses when the working tree has conflicting changes and says so.
- Pull and push actions run as cancellable jobs through the operation engine (not blocking calls), with progress and failure reported in the operation centre.
- Credential failures surface a clear error; no credentials are stored by Procyon.
- Remote-only or non-repo locations show no git controls, without errors.
- Browser and Tauri hosts behave the same (preserve parity). Network git operations must not run in the browser host unless the backend is reachable.

## Implementation Notes
- Backend first: branch list, switch, ahead/behind via `git2`, and pull/push via a job type. Keep git logic in a capability module, not in the `FileManagerService` facade.
- Frontend: extend the existing status bar in 0025; state lives in the tree, not component-local.
- Watch for the 0151 path-mismatch issue on Windows.

## Agent Notes
- Created from user request; nothing investigated yet. Confirm current 0135 API surface first.
