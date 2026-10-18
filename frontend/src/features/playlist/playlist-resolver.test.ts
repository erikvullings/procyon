import { describe, expect, it, vi } from 'vitest';
import type { EntrySummary } from '../../models/entry';
import {
  checkPlaylistRows,
  initialPlaylistRows,
  isRemoteTarget,
  resolveTargetLocation,
} from './playlist-resolver';

const playlist = { providerId: 'local', uri: 'file:///Users/me/music/mix/list.m3u8' };

describe('resolveTargetLocation', () => {
  it('resolves relative entries against the playlist directory, including parent hops', () => {
    expect(resolveTargetLocation(playlist, 'a b.mp3')?.uri).toBe(
      'file:///Users/me/music/mix/a%20b.mp3',
    );
    expect(resolveTargetLocation(playlist, '..\\other\\x.mp3')?.uri).toBe(
      'file:///Users/me/music/other/x.mp3',
    );
  });

  it('keeps absolute POSIX paths and file URLs', () => {
    expect(resolveTargetLocation(playlist, '/srv/x y.mp3')?.uri).toBe('file:///srv/x%20y.mp3');
    expect(resolveTargetLocation(playlist, 'file:///srv/x%20y.mp3')?.uri).toBe(
      'file:///srv/x%20y.mp3',
    );
  });

  it('maps Windows drive paths for local playlists only', () => {
    expect(resolveTargetLocation(playlist, 'C:\\Music\\a.mp3')?.uri).toBe('file:///C:/Music/a.mp3');
    const remote = { providerId: 'sftp', uri: 'sftp://host/home/list.m3u' };
    expect(resolveTargetLocation(remote, 'C:\\Music\\a.mp3')).toBeUndefined();
    expect(resolveTargetLocation(remote, 'songs/a.mp3')).toEqual({
      providerId: 'sftp',
      uri: 'sftp://host/home/songs/a.mp3',
    });
  });

  it('refuses UNC paths', () => {
    expect(resolveTargetLocation(playlist, '\\\\server\\share\\a.mp3')).toBeUndefined();
  });
});

describe('initialPlaylistRows', () => {
  it('classifies remote entries without a location', () => {
    expect(isRemoteTarget('https://x.test/a.mp3')).toBe(true);
    expect(isRemoteTarget('mms://x.test/a')).toBe(true);
    expect(isRemoteTarget('file:///a.mp3')).toBe(false);
    const rows = initialPlaylistRows(playlist, [
      { title: undefined, durationSeconds: undefined, target: 'http://x.test/s' },
      { title: undefined, durationSeconds: undefined, target: 'a.mp3' },
    ]);
    expect(rows.map((row) => row.status)).toEqual(['remote', 'checking']);
    expect(rows[0]?.location).toBeUndefined();
  });
});

describe('checkPlaylistRows', () => {
  it('flags targets that do not resolve or are directories', async () => {
    const rows = initialPlaylistRows(
      playlist,
      ['ok.mp3', 'gone.mp3', 'dir'].map((target) => ({
        title: undefined,
        durationSeconds: undefined,
        target,
      })),
    );
    const resolveLinkTarget = vi.fn((location: { uri: string }) => {
      if (location.uri.endsWith('gone.mp3')) return Promise.reject(new Error('not found'));
      return Promise.resolve({
        kind: location.uri.endsWith('dir') ? 'directory' : 'file',
      } as EntrySummary);
    });

    const result = await checkPlaylistRows(
      { resolveLinkTarget },
      rows,
      new AbortController().signal,
      () => undefined,
    );

    expect(result.map((row) => row.status)).toEqual(['found', 'missing', 'missing']);
  });
});
