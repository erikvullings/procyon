import type { Location } from './location';

export interface FilingRule {
  extension?: string | null;
  contentType?: string | null;
  minimumBytes?: number | null;
  folder: string;
}

export interface ApprovedMove {
  source: Location;
  folder: string;
  version?: {
    entryId: string;
    size?: number | null;
    modifiedAt?: string | null;
  } | null;
}

export interface FilingStatus {
  assignment: ApprovedMove;
  dueAt: string;
  operationId?: string | null;
  error?: string | null;
}

export interface WatchedFolder {
  root: Location;
  delayHours: number;
  paused: boolean;
  rules: FilingRule[];
  pending: FilingStatus[];
  error?: string | null;
}

export interface OrganizerPreview {
  moves: ApprovedMove[];
  unmatched: Location[];
  unprocessed: Location[];
}

export type OrganizerRequest =
  | { action: 'list' }
  | { action: 'preview'; root: Location }
  | { action: 'collect' | 'cancel'; jobId: string }
  | {
      action: 'save';
      root: Location;
      delayHours: number;
      paused: boolean;
      rules: FilingRule[];
      approved: ApprovedMove[];
    }
  | { action: 'remove'; root: Location };

export interface OrganizerResponse {
  folders: WatchedFolder[];
  jobId?: string | null;
  preview?: OrganizerPreview | null;
}
