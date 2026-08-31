import type { DirectorySnapshotDto } from '../api/generated/models/directorySnapshotDto';
import { type EntrySummary, entrySummaryFromDto } from './entry';
import type { EntryId, PaneId } from './ids';
import type { Location } from './location';

/**
 * The loading state of a pane's directory listing, mirroring
 * `fm_transport_dto::LoadingStateDto`'s `type` tag.
 */
export type LoadingState =
  | { type: 'idle' }
  | { type: 'loading' }
  | { type: 'loaded' }
  | { type: 'error'; message: string };

/**
 * Total/available capacity for the volume backing a directory snapshot's
 * location (task 0096), mirroring `fm_transport_dto::VolumeCapacityDto`.
 */
export interface VolumeCapacity {
  totalBytes: number;
  availableBytes: number;
}

/**
 * A batch of directory entries for one pane, at a specific revision (spec
 * §5.4), mirroring `fm_transport_dto::DirectorySnapshotDto`.
 */
export interface DirectorySnapshot {
  paneId: PaneId;
  requestId: string;
  revision: number;
  location: Location;
  /** Whether the current user may create entries in this directory. */
  writable: boolean;
  entries: EntrySummary[];
  totalKnownEntries?: number;
  /** Combined byte size of every file/symlink entry, when known in advance. */
  totalKnownSize?: number;
  /** Number of file/symlink entries (directories excluded), when known in advance. */
  totalKnownFileCount?: number;
  hasMore: boolean;
  continuationToken?: string;
  loadingState: LoadingState;
  /** Backing volume's total/available capacity, when known (task 0096). */
  volumeCapacity?: VolumeCapacity;
}

/**
 * Converts the wire DTO into the frontend model, normalizing the wire's `null` (used for
 * absent optional fields) to `undefined`, and mapping each entry through
 * {@link entrySummaryFromDto}.
 */
export function directorySnapshotFromDto(dto: DirectorySnapshotDto): DirectorySnapshot {
  return {
    paneId: dto.paneId,
    requestId: dto.requestId,
    revision: dto.revision,
    location: dto.location,
    writable: dto.writable,
    entries: dto.entries.map(entrySummaryFromDto),
    hasMore: dto.hasMore,
    loadingState: dto.loadingState,
    ...(dto.totalKnownEntries == null ? {} : { totalKnownEntries: dto.totalKnownEntries }),
    ...(dto.totalKnownSize == null ? {} : { totalKnownSize: dto.totalKnownSize }),
    ...(dto.totalKnownFileCount == null ? {} : { totalKnownFileCount: dto.totalKnownFileCount }),
    ...(dto.continuationToken == null ? {} : { continuationToken: dto.continuationToken }),
    ...(dto.volumeCapacity == null
      ? {}
      : {
          volumeCapacity: {
            totalBytes: dto.volumeCapacity.totalBytes,
            availableBytes: dto.volumeCapacity.availableBytes,
          },
        }),
  };
}

/**
 * An incremental change to a previously delivered {@link DirectorySnapshot}
 * (spec §5.4); delivered over the event stream (task 0014/0032), never
 * fetched via a request/response call.
 */
export type DirectoryDelta =
  | { type: 'entriesAdded'; revision: number; entries: EntrySummary[] }
  | { type: 'entriesUpdated'; revision: number; entries: EntrySummary[] }
  | { type: 'entriesRemoved'; revision: number; entryIds: EntryId[] }
  | { type: 'reset'; snapshot: DirectorySnapshot };
