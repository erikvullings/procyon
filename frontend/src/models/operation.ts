import type { OperationId } from './ids';
import type { EntryRef, Location } from './location';

/** Initial operation kinds (spec §17). */
export type OperationKind =
  | 'createArchive'
  | 'moveToArchive'
  | 'createDirectory'
  | 'createFile'
  | 'rename'
  | 'copy'
  | 'move'
  | 'duplicate'
  | 'trash'
  | 'delete'
  | 'search'
  | 'compare';

/** Operation lifecycle states (spec §17). */
export type OperationState =
  | 'queued'
  | 'planning'
  | 'running'
  | 'paused'
  | 'waitingForConflictResolution'
  | 'cancelling'
  | 'cancelled'
  | 'completed'
  | 'completedWithWarnings'
  | 'failed'
  | 'interrupted';

/** Conflict policies (spec §17). Only ask/skip/overwrite/renameNew are reliably implemented initially. */
export type ConflictPolicy = 'ask' | 'skip' | 'overwrite' | 'renameNew' | 'keepNewer';

/** Progress for a running operation (spec §17). */
export interface OperationProgress {
  completedItems: number;
  totalItems?: number;
  completedBytes: number;
  totalBytes?: number;
  currentEntry?: EntryRef;
  bytesPerSecond?: number;
}

/**
 * A mutating file operation represented as a job (spec §17). No backend DTO
 * exists yet (the operation engine lands in tasks 0037+); fields mirror the
 * domain `Operation` struct until then.
 */
export interface Operation {
  id: OperationId;
  kind: OperationKind;
  state: OperationState;
  sources: readonly EntryRef[];
  destination?: Location;
  progress: OperationProgress;
  conflictPolicy: ConflictPolicy;
  createdAt: string;
  startedAt?: string;
  completedAt?: string;
  /** One-based scheduler position while the operation is queued. */
  queuePosition?: number;
  /** Backend-provided completion summary retained in the operation centre. */
  result?: OperationResult;
  /** Entry-level warnings collected for completed-with-warnings operations. */
  errors?: readonly OperationEntryError[];
}

export interface OperationResult {
  message: string;
  details?: Readonly<Record<string, unknown>>;
}

export interface OperationFailure {
  code: string;
  message: string;
  details?: Readonly<Record<string, unknown>>;
}

export interface OperationEntryError {
  entry: EntryRef;
  message: string;
}
