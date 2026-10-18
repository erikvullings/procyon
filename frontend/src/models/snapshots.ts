import type { SnapshotDiffCountsDto } from '../api/generated/models/snapshotDiffCountsDto';
import type { SnapshotDiffEntryDto } from '../api/generated/models/snapshotDiffEntryDto';
import type { SnapshotJobPageDto } from '../api/generated/models/snapshotJobPageDto';
import type { Location } from './location';

export type SnapshotDiffCounts = SnapshotDiffCountsDto;
export type SnapshotDiffEntry = SnapshotDiffEntryDto;
export type SnapshotJobPage = SnapshotJobPageDto;

/** Starts a snapshot create or compare job (task 0268). */
export type StartSnapshotJobRequest =
  | {
      readonly type: 'create';
      readonly root: Location;
      readonly destination: Location;
      readonly withChecksums: boolean;
      readonly showHidden?: boolean;
    }
  | {
      readonly type: 'compare';
      readonly snapshot: Location;
      readonly target: Location;
      readonly verifyChecksums?: boolean;
      readonly showHidden?: boolean;
    };

export interface StartSnapshotJobResult {
  readonly jobId: string;
}
