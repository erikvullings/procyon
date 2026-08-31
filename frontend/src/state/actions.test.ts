import { describe, expect, it, vi } from 'vitest';

import type { Operation } from '../models';
import { createAppActions } from './actions';
import { createInitialAppState } from './model';
import { createAppStore } from './store';

describe('AppActions', () => {
  it('routes independent producers through the store frame batch', () => {
    const frames: FrameRequestCallback[] = [];
    const redraw = vi.fn();
    const store = createAppStore(createInitialAppState('mock'), {
      requestFrame: (callback) => {
        frames.push(callback);
        return frames.length;
      },
      redraw,
    });
    const actions = createAppActions(store.update);
    const operation: Operation = {
      id: 'operation-1',
      kind: 'copy',
      state: 'running',
      sources: [],
      progress: { completedItems: 0, completedBytes: 0 },
      conflictPolicy: 'ask',
      createdAt: '2026-07-30T00:00:00Z',
    };

    actions.setConnection({ status: 'open' });
    actions.upsertOperation(operation);
    actions.updateOperationProgress('operation-1', {
      completedItems: 4,
      completedBytes: 256,
    });

    expect(frames).toHaveLength(1);
    frames[0]?.(0);
    expect(store.getState().connection.status).toBe('open');
    expect(store.getState().operations.byId['operation-1']?.progress.completedItems).toBe(4);
    expect(redraw).toHaveBeenCalledTimes(1);
  });
});
