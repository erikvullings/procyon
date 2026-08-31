import type { ConflictResolution, OperationId } from '../../models';

export interface ConflictDialogState {
  readonly operationId: OperationId;
  readonly conflictId: string;
  readonly resolution?: Exclude<ConflictResolution, 'confirm'>;
  readonly applyToAllSimilar: boolean;
}

export type ConflictDialogAction =
  | { readonly type: 'open'; readonly operationId: OperationId; readonly conflictId: string }
  | { readonly type: 'choose'; readonly resolution: Exclude<ConflictResolution, 'confirm'> }
  | { readonly type: 'toggleApplyToAll' }
  | { readonly type: 'close' };

export function reduceConflictDialog(
  state: ConflictDialogState | undefined,
  action: ConflictDialogAction,
): ConflictDialogState | undefined {
  switch (action.type) {
    case 'open':
      return {
        operationId: action.operationId,
        conflictId: action.conflictId,
        applyToAllSimilar: false,
      };
    case 'choose':
      return state === undefined ? undefined : { ...state, resolution: action.resolution };
    case 'toggleApplyToAll':
      return state === undefined
        ? undefined
        : { ...state, applyToAllSimilar: !state.applyToAllSimilar };
    case 'close':
      return undefined;
  }
}
