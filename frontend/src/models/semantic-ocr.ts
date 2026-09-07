import type { Location } from './location';

/** Why a safely discovered OCRmyPDF installation cannot be used. */
export type SemanticOcrUnavailableReason =
  | { readonly code: 'hostUnavailable' }
  | { readonly code: 'missing' }
  | { readonly code: 'nonExecutable'; readonly path: string }
  | { readonly code: 'couldNotExecute' }
  | { readonly code: 'malformedVersion' }
  | { readonly code: 'unsupportedVersion'; readonly version: string }
  | { readonly code: 'timedOut' }
  | { readonly code: 'outputTooLarge'; readonly limit: number };

/** Safe backend discovery result. */
export type SemanticOcrAvailability =
  | {
      readonly state: 'available';
      readonly executable: string;
      readonly version: string;
    }
  | {
      readonly state: 'unavailable';
      readonly reason: SemanticOcrUnavailableReason;
      readonly guidance: string;
    };

/** One backend-reported OCR remediation target. */
export interface SemanticOcrTarget {
  readonly rootId: string;
  readonly location: Location;
}

export type SemanticOcrJobState = 'queued' | 'running' | 'completed' | 'failed' | 'cancelled';

/** Typed per-file result shown without parsing diagnostics in the UI. */
export type SemanticOcrOutcome =
  | { readonly outcome: 'succeeded' }
  | { readonly outcome: 'postOcrNoText'; readonly detail: string }
  | { readonly outcome: 'executionFailure'; readonly detail: string }
  | { readonly outcome: 'skipped'; readonly detail: string };

export interface SemanticOcrFileOutcome {
  readonly rootId: string;
  readonly location: Location;
  readonly outcome: SemanticOcrOutcome;
}

/** Durable remediation job snapshot. */
export interface SemanticOcrJob {
  readonly id: string;
  readonly state: SemanticOcrJobState;
  readonly createdAtMs: number;
  readonly updatedAtMs: number;
  readonly totalFiles: number;
  readonly processedFiles: number;
  readonly files: readonly SemanticOcrFileOutcome[];
  readonly availabilityFailure: SemanticOcrUnavailableReason | null;
}

/** Complete OCR Settings projection. */
export interface SemanticOcrStatus {
  readonly enabled: boolean;
  readonly availability: SemanticOcrAvailability;
  readonly reportedFiles: readonly SemanticOcrTarget[];
  readonly jobs: readonly SemanticOcrJob[];
}

/** The four user-visible remediation scopes. */
export type StartSemanticOcrRemediationRequest =
  | { readonly scope: 'oneFile'; readonly file: SemanticOcrTarget }
  | { readonly scope: 'selectedFiles'; readonly files: readonly SemanticOcrTarget[] }
  | { readonly scope: 'enrolledRoot'; readonly rootId: string }
  | { readonly scope: 'allReported' };

/** Structured Tauri failure. */
export interface SemanticOcrError {
  readonly code:
    | 'disabled'
    | 'unavailable'
    | 'notFound'
    | 'nothingToRemediate'
    | 'unreportedTarget'
    | 'queueFull'
    | 'tooManyTargets'
    | 'invalidRequest'
    | 'persist'
    | 'workerRestart';
  readonly message: string;
  readonly availabilityReason: SemanticOcrUnavailableReason | null;
  readonly maximum: number | null;
}
