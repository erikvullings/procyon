import { describe, expect, it } from 'vitest';
import type { ResolvedPlaylistTrack } from '../playlist/playlist-resolver';
import { createQueueItem, queueItemsFromPlaylist } from './queue-items';

const location = { providerId: 'local', uri: 'file:///m/a.mp3' };

function track(
  status: ResolvedPlaylistTrack['status'],
  withLocation = true,
): ResolvedPlaylistTrack {
  return {
    title: undefined,
    durationSeconds: 10,
    target: 'a.mp3',
    status,
    location: withLocation ? location : undefined,
  };
}

describe('queue items', () => {
  it('gives every created item a unique id so a file can be queued twice', () => {
    expect(createQueueItem(location, 'a', undefined).id).not.toBe(
      createQueueItem(location, 'a', undefined).id,
    );
  });

  it('queues found and still-checking rows and counts the rest', () => {
    const result = queueItemsFromPlaylist(
      [track('found'), track('missing'), track('remote', false), track('checking')],
      () => 'T',
    );
    expect(result.items).toHaveLength(2);
    expect([...result.itemIdByRow.keys()]).toEqual([0, 3]);
    expect(result.missingSkipped).toBe(1);
    expect(result.remoteSkipped).toBe(1);
  });
});
