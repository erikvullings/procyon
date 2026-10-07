import type {
  BackendEvent,
  Operation,
  OperationFailure,
  OperationId,
  OperationState,
} from '../../models';

export interface OperationCentreState {
  readonly byId: Readonly<Partial<Record<OperationId, Operation>>>;
  readonly failuresById: Readonly<Partial<Record<OperationId, OperationFailure>>>;
}

export function createOperationsState(operations: readonly Operation[] = []): OperationCentreState {
  return {
    byId: Object.fromEntries(operations.map((operation) => [operation.id, operation])),
    failuresById: {},
  };
}

/** Restores history without replacing newer event-driven operation snapshots. */
export function mergeOperationHistory(
  state: OperationCentreState,
  history: readonly Operation[],
): OperationCentreState {
  const restored = createOperationsState(history);
  return {
    byId: { ...restored.byId, ...state.byId },
    failuresById: { ...restored.failuresById, ...state.failuresById },
  };
}

/** Applies an event-stream batch atomically; the operation centre never polls. */
export function reduceOperationEvents(
  state: OperationCentreState,
  events: readonly BackendEvent[],
): OperationCentreState {
  const byId = { ...state.byId };
  const failuresById = { ...state.failuresById };
  for (const { payload } of events) {
    switch (payload.type) {
      case 'operation.created':
      case 'operation.completed':
        byId[payload.operation.id] = payload.operation;
        break;
      case 'operation.progress': {
        const current = byId[payload.operationId];
        if (current !== undefined) {
          byId[payload.operationId] = {
            ...current,
            progress: { ...current.progress, ...payload.progress },
          };
        }
        break;
      }
      case 'operation.stateChanged': {
        const current = byId[payload.operationId];
        if (current !== undefined) {
          byId[payload.operationId] = { ...current, state: payload.state };
        }
        break;
      }
      case 'operation.failed': {
        const current = byId[payload.operationId];
        if (current !== undefined) {
          byId[payload.operationId] = { ...current, state: 'failed' };
          failuresById[payload.operationId] = {
            code: payload.code,
            message: payload.message,
            ...(payload.details === undefined ? {} : { details: payload.details }),
          };
        }
        break;
      }
      default:
        break;
    }
  }
  return { byId, failuresById };
}

/** Applies an acknowledged UI transition while the backend command is in flight. */
export function transitionOperationState(
  state: OperationCentreState,
  operationId: OperationId,
  operationState: OperationState,
): OperationCentreState {
  const operation = state.byId[operationId];
  if (operation === undefined) return state;
  return {
    ...state,
    byId: {
      ...state.byId,
      [operationId]: { ...operation, state: operationState },
    },
  };
}

export function dismissOperation(
  state: OperationCentreState,
  operationId: OperationId,
): OperationCentreState {
  const byId = { ...state.byId };
  const failuresById = { ...state.failuresById };
  delete byId[operationId];
  delete failuresById[operationId];
  return { byId, failuresById };
}

/** Undoable completions remain visible until the user invokes or explicitly dismisses them. */
export function shouldAutoDismissOperation(operation: Operation): boolean {
  return (
    operation.undo?.available !== true &&
    (operation.state === 'completed' ||
      operation.state === 'completedWithWarnings' ||
      operation.state === 'cancelled' ||
      operation.state === 'interrupted')
  );
}

export function isActiveOperation(operation: Operation): boolean {
  return (
    operation.state === 'queued' ||
    operation.state === 'planning' ||
    operation.state === 'running' ||
    operation.state === 'paused' ||
    operation.state === 'waitingForConflictResolution' ||
    operation.state === 'cancelling'
  );
}

export interface ActiveOperationsSummary {
  readonly count: number;
  /** Overall completion (0-100) when every active job reports a byte or item total. */
  readonly percent?: number;
}

/** Aggregates active jobs for the compact toolbar progress indicator. */
export function summariseActiveOperations(
  state: OperationCentreState,
): ActiveOperationsSummary | undefined {
  let count = 0;
  let done = 0;
  let total = 0;
  let measurable = true;
  for (const operation of Object.values(state.byId)) {
    if (operation === undefined || !isActiveOperation(operation)) continue;
    count += 1;
    const { completedBytes, totalBytes, completedItems, totalItems } = operation.progress;
    if (totalBytes !== undefined && totalBytes > 0) {
      done += Math.min(completedBytes, totalBytes) / totalBytes;
      total += 1;
    } else if (totalItems !== undefined && totalItems > 0) {
      done += Math.min(completedItems, totalItems) / totalItems;
      total += 1;
    } else {
      measurable = false;
    }
  }
  if (count === 0) return undefined;
  if (!measurable || total === 0) return { count };
  return { count, percent: Math.round((done / total) * 100) };
}
