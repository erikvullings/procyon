import { describe, expect, it, vi } from 'vitest';

import type { FileManagerClient } from '../../api/client/file-manager-client';
import { ApiError } from '../../api/fetch-mutator';
import type { WorkspaceCommand, WorkspaceProjection } from '../../models';
import { dispatchWorkspaceCommand } from './dispatch-workspace-command';

function projection(revision: number, name = 'Workspace'): WorkspaceProjection {
  return {
    id: 'workspace-1',
    name,
    revision,
    layout: { type: 'pane', paneId: 'pane-1' },
    paneOrder: ['pane-1'],
    panesById: {
      'pane-1': {
        id: 'pane-1',
        tabOrder: ['tab-1'],
        tabsById: {
          'tab-1': {
            id: 'tab-1',
            title: 'tmp',
            location: { providerId: 'local', uri: 'file:///tmp' },
            canNavigateBack: false,
            canNavigateForward: false,
            view: {
              sort: [],
              columns: [],
              showHidden: false,
              foldersFirst: true,
              quickFilter: null,
            },
          },
        },
        activeTabId: 'tab-1',
      },
    },
    activePaneId: 'pane-1',
    operationCentre: { visible: false, height: 180 },
    ephemeral: false,
  };
}

function clientWith(
  dispatch: FileManagerClient['dispatchWorkspaceCommand'],
  getWorkspace: FileManagerClient['getWorkspace'],
): FileManagerClient {
  return {
    dispatchWorkspaceCommand: dispatch,
    getWorkspace,
  } as FileManagerClient;
}

describe('dispatchWorkspaceCommand', () => {
  it('reloads and retries a safely idempotent command at the latest revision', async () => {
    const conflict = new ApiError(409, {
      code: 'workspaceRevisionConflict',
      message: 'stale',
      details: { workspaceId: 'workspace-1', expectedRevision: 4, actualRevision: 5 },
    });
    const latest = projection(5);
    const changed = projection(6, 'Renamed');
    const dispatch = vi
      .fn<FileManagerClient['dispatchWorkspaceCommand']>()
      .mockRejectedValueOnce(conflict)
      .mockResolvedValueOnce(changed);
    const getWorkspace = vi.fn<FileManagerClient['getWorkspace']>().mockResolvedValue(latest);
    const command: WorkspaceCommand = {
      type: 'renameWorkspace',
      workspaceId: 'workspace-1',
      expectedRevision: 4,
      name: 'Renamed',
    };
    const projections: WorkspaceProjection[] = [];

    const result = await dispatchWorkspaceCommand(
      clientWith(dispatch, getWorkspace),
      command,
      (next) => projections.push(next),
    );

    expect(result).toEqual(changed);
    expect(getWorkspace).toHaveBeenCalledWith('workspace-1', undefined);
    expect(dispatch).toHaveBeenNthCalledWith(2, { ...command, expectedRevision: 5 }, undefined);
    expect(projections).toEqual([latest, changed]);
  });

  it('reloads but surfaces a non-idempotent stale command without silently retrying', async () => {
    const conflict = new ApiError(409, {
      code: 'workspaceRevisionConflict',
      message: 'stale',
      details: { workspaceId: 'workspace-1', expectedRevision: 4, actualRevision: 5 },
    });
    const latest = projection(5);
    const dispatch = vi
      .fn<FileManagerClient['dispatchWorkspaceCommand']>()
      .mockRejectedValue(conflict);
    const getWorkspace = vi.fn<FileManagerClient['getWorkspace']>().mockResolvedValue(latest);
    const command: WorkspaceCommand = {
      type: 'addTab',
      workspaceId: 'workspace-1',
      expectedRevision: 4,
      paneId: 'pane-1',
      location: { providerId: 'local', uri: 'file:///tmp' },
    };
    const projections: WorkspaceProjection[] = [];

    await expect(
      dispatchWorkspaceCommand(clientWith(dispatch, getWorkspace), command, (next) =>
        projections.push(next),
      ),
    ).rejects.toBe(conflict);

    expect(dispatch).toHaveBeenCalledTimes(1);
    expect(projections).toEqual([latest]);
  });
});
