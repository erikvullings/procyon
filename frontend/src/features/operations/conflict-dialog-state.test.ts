import { describe, expect, it } from 'vitest';
import { reduceConflictDialog } from './conflict-dialog-state';

describe('conflict dialog state machine', () => {
  it('opens, chooses a decision, and toggles apply-to-all independently', () => {
    let state = reduceConflictDialog(undefined, {
      type: 'open',
      operationId: 'operation-1',
      conflictId: 'conflict-1',
    });
    state = reduceConflictDialog(state, { type: 'choose', resolution: 'overwrite' });
    expect(state?.resolution).toBe('overwrite');
    state = reduceConflictDialog(state, { type: 'toggleApplyToAll' });
    expect(state?.applyToAllSimilar).toBe(true);
  });

  it('offers skip, rename, and cancel as explicit choices and closes without a decision', () => {
    for (const resolution of ['skip', 'renameNew', 'cancelOperation'] as const) {
      const state = reduceConflictDialog(
        reduceConflictDialog(undefined, { type: 'open', operationId: 'op', conflictId: 'c' }),
        { type: 'choose', resolution },
      );
      expect(state?.resolution).toBe(resolution);
    }
    expect(reduceConflictDialog(undefined, { type: 'choose', resolution: 'skip' })).toBeUndefined();
    expect(
      reduceConflictDialog(
        { operationId: 'op', conflictId: 'c', applyToAllSimilar: false },
        { type: 'close' },
      ),
    ).toBeUndefined();
  });
});
