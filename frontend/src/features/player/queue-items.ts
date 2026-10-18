import type { Location } from '../../models/location';
import type { ResolvedPlaylistTrack } from '../playlist/playlist-resolver';
import type { QueueItem } from './player-state';

let nextQueueItemNumber = 0;

/** A queue item with a process-unique id, so the same file can be queued more than once. */
export function createQueueItem(
  location: Location,
  title: string,
  durationSeconds: number | undefined,
): QueueItem {
  nextQueueItemNumber += 1;
  return { id: `queue-item-${nextQueueItemNumber}`, title, location, durationSeconds };
}

export interface PlaylistQueueItems {
  readonly items: readonly QueueItem[];
  /** Queue item created for each playlist row that is playable, keyed by row index. */
  readonly itemIdByRow: ReadonlyMap<number, string>;
  /** Network streams that were not queued (never fetched without an opt-in). */
  readonly remoteSkipped: number;
  /** Rows whose target does not exist. */
  readonly missingSkipped: number;
}

/** Queues the playable rows of a playlist; missing files and network streams are counted, not queued. */
export function queueItemsFromPlaylist(
  tracks: readonly ResolvedPlaylistTrack[],
  titleFor: (track: ResolvedPlaylistTrack) => string,
): PlaylistQueueItems {
  const items: QueueItem[] = [];
  const itemIdByRow = new Map<number, string>();
  let remoteSkipped = 0;
  let missingSkipped = 0;
  tracks.forEach((track, row) => {
    if (track.status === 'remote') remoteSkipped += 1;
    else if (track.status === 'missing') missingSkipped += 1;
    else if (track.location !== undefined) {
      const item = createQueueItem(track.location, titleFor(track), track.durationSeconds);
      items.push(item);
      itemIdByRow.set(row, item.id);
    }
  });
  return { items, itemIdByRow, remoteSkipped, missingSkipped };
}
