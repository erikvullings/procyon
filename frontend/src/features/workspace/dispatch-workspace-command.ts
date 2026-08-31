import type { FileManagerClient } from '../../api/client/file-manager-client';
import { ApiError } from '../../api/fetch-mutator';
import type { WorkspaceCommand, WorkspaceProjection } from '../../models';

/** Client surface required to dispatch and, on conflict, reconcile a workspace command. */
export type WorkspaceCommandClient = Pick<
  FileManagerClient,
  'dispatchWorkspaceCommand' | 'getWorkspace'
>;

type ReplaceProjection = (projection: WorkspaceProjection) => void;

function isSafelyIdempotent(command: WorkspaceCommand): boolean {
  switch (command.type) {
    case 'renameWorkspace':
    case 'setActivePane':
    case 'activateTab':
    case 'updateView':
    case 'updateLayout':
      return true;
    case 'navigateTab':
      return command.navigationMode === 'refresh';
    case 'addTab':
    case 'addTransientTab':
    case 'closeTab':
    case 'moveTab':
      return false;
  }
}

/** True for a stale-projection conflict raised by any workspace-mutating client call. */
export function isWorkspaceRevisionConflict(error: unknown): error is ApiError {
  return error instanceof ApiError && error.code === 'workspaceRevisionConflict';
}

/**
 * Dispatches one semantic mutation, reconciling stale projections without
 * replaying commands whose effects could be duplicated.
 */
export async function dispatchWorkspaceCommand(
  client: WorkspaceCommandClient,
  command: WorkspaceCommand,
  replaceProjection: ReplaceProjection,
  signal?: AbortSignal,
): Promise<WorkspaceProjection> {
  try {
    const changed = await client.dispatchWorkspaceCommand(command, signal);
    replaceProjection(changed);
    return changed;
  } catch (error) {
    if (!isWorkspaceRevisionConflict(error)) {
      throw error;
    }
    const latest = await client.getWorkspace(command.workspaceId, signal);
    replaceProjection(latest);
    if (!isSafelyIdempotent(command)) {
      throw error;
    }
    const changed = await client.dispatchWorkspaceCommand(
      { ...command, expectedRevision: latest.revision },
      signal,
    );
    replaceProjection(changed);
    return changed;
  }
}
