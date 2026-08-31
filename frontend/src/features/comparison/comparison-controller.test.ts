import { beforeEach, describe, expect, it, vi } from 'vitest';

import { MockFileManagerClient } from '../../api/client/mock-file-manager-client';
import type { WorkspaceProjection } from '../../models';
import {
  type ComparisonController,
  type ComparisonControllerContext,
  createComparisonController,
} from './comparison-controller';
import { type ComparisonState, initialComparisonState } from './comparison-state';

function workspace(): WorkspaceProjection {
  return {
    id: 'workspace-1',
    name: 'Workspace',
    revision: 1,
    layout: {
      type: 'split',
      axis: 'horizontal',
      ratio: 0.5,
      first: { type: 'pane', paneId: 'pane-left' },
      second: { type: 'pane', paneId: 'pane-right' },
    },
    paneOrder: ['pane-left', 'pane-right'],
    activePaneId: 'pane-left',
    operationCentre: { visible: true, height: 200 },
    ephemeral: false,
    panesById: {
      'pane-left': {
        id: 'pane-left',
        tabOrder: ['tab-left'],
        activeTabId: 'tab-left',
        tabsById: {
          'tab-left': {
            id: 'tab-left',
            title: 'Left',
            location: { providerId: 'local', uri: 'file:///left' },
            canNavigateBack: false,
            canNavigateForward: false,
            view: { sort: [], columns: [], showHidden: false, foldersFirst: true },
          },
        },
      },
      'pane-right': {
        id: 'pane-right',
        tabOrder: ['tab-right'],
        activeTabId: 'tab-right',
        tabsById: {
          'tab-right': {
            id: 'tab-right',
            title: 'Right',
            location: { providerId: 'local', uri: 'file:///right' },
            canNavigateBack: false,
            canNavigateForward: false,
            view: { sort: [], columns: [], showHidden: false, foldersFirst: true },
          },
        },
      },
    },
  };
}

describe('ComparisonController', () => {
  let client: MockFileManagerClient;
  let state: ComparisonState;
  const redraw = vi.fn();
  let context: ComparisonControllerContext;
  let controller: ComparisonController;

  beforeEach(() => {
    client = new MockFileManagerClient();
    state = initialComparisonState();
    redraw.mockReset();
    context = {
      getState: () => state,
      setState: (next) => {
        state = next;
      },
      getWorkspace: () => workspace(),
      getClient: () => client,
      redraw,
    };
    controller = createComparisonController(context);
  });

  it('starts a comparison between the first two panes and records the started state', async () => {
    vi.spyOn(client, 'startComparison');

    controller.startComparison('sizeAndTimestamp');
    await vi.waitFor(() => expect(state.comparisonId).toBeDefined());

    expect(client.startComparison).toHaveBeenCalledWith({
      workspaceId: 'workspace-1',
      left: { providerId: 'local', uri: 'file:///left' },
      right: { providerId: 'local', uri: 'file:///right' },
      criteria: 'sizeAndTimestamp',
    });
    expect(state.leftPaneId).toBe('pane-left');
    expect(state.rightPaneId).toBe('pane-right');
    expect(state.leftRoot).toEqual({ providerId: 'local', uri: 'file:///left' });
    expect(redraw).toHaveBeenCalled();
  });

  it('records an error and never calls the client when fewer than two panes are open', () => {
    context.getWorkspace = () => {
      const single = workspace();
      return { ...single, paneOrder: ['pane-left'] };
    };
    vi.spyOn(client, 'startComparison');

    controller.startComparison('nameOnly');

    expect(client.startComparison).not.toHaveBeenCalled();
    expect(state.error).toMatch(/two open panes/);
  });

  it('cancels the active comparison and clears its state', async () => {
    controller.startComparison('nameOnly');
    await vi.waitFor(() => expect(state.comparisonId).toBeDefined());
    vi.spyOn(client, 'cancelComparison');

    const activeId = state.comparisonId;
    controller.cancelComparison();

    expect(client.cancelComparison).toHaveBeenCalledWith(activeId);
    expect(state.comparisonId).toBeUndefined();
  });

  it('cancelComparison is a no-op when nothing is running', () => {
    vi.spyOn(client, 'cancelComparison');

    controller.cancelComparison();

    expect(client.cancelComparison).not.toHaveBeenCalled();
  });

  it('toggles differencesOnly', () => {
    controller.setDifferencesOnly(true);
    expect(state.differencesOnly).toBe(true);
    controller.setDifferencesOnly(false);
    expect(state.differencesOnly).toBe(false);
  });

  it('handleResultsBatch merges entries into the running comparison', async () => {
    controller.startComparison('nameOnly');
    await vi.waitFor(() => expect(state.comparisonId).toBeDefined());
    const comparisonId = state.comparisonId;
    if (comparisonId === undefined) throw new Error('comparison must have started');

    controller.handleResultsBatch(
      comparisonId,
      [{ relativePath: 'a.txt', status: 'onlyLeft' }],
      true,
      0,
    );

    expect(state.statusByRelativePath.get('a.txt')?.status).toBe('onlyLeft');
    expect(state.isComplete).toBe(true);
  });

  it('starting a new comparison cancels the previous one first', async () => {
    controller.startComparison('nameOnly');
    await vi.waitFor(() => expect(state.comparisonId).toBeDefined());
    const firstId = state.comparisonId;
    vi.spyOn(client, 'cancelComparison');

    controller.startComparison('contentHash');
    await vi.waitFor(() => expect(state.criteria).toBe('contentHash'));

    expect(client.cancelComparison).toHaveBeenCalledWith(firstId);
  });
});
