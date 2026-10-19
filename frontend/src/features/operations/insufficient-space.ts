import type { Operation, OperationFailure, StartOperationRequest } from '../../models';

/** An operation that failed its disk-space preflight, with the numbers to show (task 0272). */
export interface SpaceShortfall {
  readonly operation: Operation;
  readonly required: number;
  readonly available: number;
  readonly shortfall: number;
}

const RETRYABLE_TYPES: ReadonlySet<Operation['kind']> = new Set(['copy', 'move', 'duplicate']);

function bytesOf(value: unknown): number | undefined {
  return typeof value === 'number' && Number.isFinite(value) && value >= 0 ? value : undefined;
}

/** Reads a failure as a space shortfall; undefined for other failures or missing numbers. */
export function spaceShortfallOf(
  operation: Operation,
  failure: OperationFailure | undefined,
): SpaceShortfall | undefined {
  if (failure?.code !== 'insufficientSpace' || !RETRYABLE_TYPES.has(operation.kind)) {
    return undefined;
  }
  const required = bytesOf(failure.details?.required);
  const available = bytesOf(failure.details?.available);
  if (required === undefined || available === undefined || required <= available) return undefined;
  return { operation, required, available, shortfall: required - available };
}

/** The request that repeats `operation` with the disk-space check skipped. */
export function requestSkippingSpaceCheck(operation: Operation): StartOperationRequest {
  return {
    type: operation.kind as StartOperationRequest['type'],
    sources: operation.sources.map((source) => source.location),
    ...(operation.destination === undefined ? {} : { destination: operation.destination }),
    conflictPolicy: operation.conflictPolicy,
    skipSpaceCheck: true,
  };
}
