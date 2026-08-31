# 0117 Deepen Connections Model with Full Lifecycle

Status: done
Priority: medium
Subsystem: frontend
Depends on: none

## Context

The connections module (`connections-model.ts`) has real substance in its first half (SFTP URI construction, status labels, validation, `isBrowsable`) but its second half (lines 164-242) are one-line pass-throughs to `FileManagerClient`: `createConnection()`, `updateConnection()`, `deleteConnection()`, `listConnections()`. The comment says "components must depend only on this module" — establishing a seam — but the module adds no behaviour at that seam. The deletion test: deleting these CRUD wrappers would just push the `client.listConnections()` calls to callers. No complexity is hidden.

## Acceptance Criteria

- Connection model owns full lifecycle: validation before save, error mapping from server responses, connection testing with result parsing
- Loading state and error state centralized so callers don't reconstruct try/catch patterns
- CRUD pass-throughs either deepened with added behavior (validation, error mapping) or removed entirely with callers using the client directly
- Components still depend only on the connections model — the seam remains
- Tests for validation, error mapping, and lifecycle using `MockFileManagerClient`

## Implementation Notes

- `frontend/src/features/connections/connections-model.ts` (243 lines)
- `frontend/src/features/connections/connection-editor.ts` — primary caller
- `frontend/src/api/client/file-manager-client.ts` — connection CRUD interface
- The seam exists. Give it depth.
- Reference: architecture review — deepening opportunity #6

## Agent Notes

- 2026-08-10 claude: Added ConnectionSaveDraft, ConnectionSaveResult, ConnectionLifecycle types and saveConnection lifecycle to connections-model.ts. Removed createConnection/updateConnection pass-throughs. connection-editor.ts updated to use onSave callback (replaced onCreate/onUpdate). app-shell.ts wired onSave to saveConnection. 10 new tests. TypeScript clean (pre-existing errors only), all connection tests pass (34 total).
