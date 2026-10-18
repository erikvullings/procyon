import m from 'mithril';
import { describe, expect, it, vi } from 'vitest';
import type { FileViewerPlaylistContent } from '../preview/file-viewer-controller';
import { PlaylistView } from './playlist-view';

const content: FileViewerPlaylistContent = {
  kind: 'playlist',
  format: 'm3u8',
  truncated: false,
  error: undefined,
  tracks: [
    {
      title: 'One',
      durationSeconds: 65,
      target: 'one.mp3',
      status: 'found',
      location: { providerId: 'local', uri: 'file:///m/one.mp3' },
    },
    {
      title: undefined,
      durationSeconds: undefined,
      target: 'gone.mp3',
      status: 'missing',
      location: { providerId: 'local', uri: 'file:///m/gone.mp3' },
    },
    {
      title: undefined,
      durationSeconds: undefined,
      target: 'http://x.test/s',
      status: 'remote',
      location: undefined,
    },
  ],
};

describe('PlaylistView', () => {
  it('lists tracks with duration, flags missing files and labels network streams', () => {
    const root = document.createElement('div');
    m.render(root, m(PlaylistView, { content }));

    const rows = Array.from(root.querySelectorAll('.fm-playlist-row'));
    expect(rows).toHaveLength(3);
    expect(rows[0]?.textContent).toContain('One');
    expect(rows[0]?.textContent).toContain('1:05');
    expect(rows[1]?.classList.contains('fm-playlist-missing')).toBe(true);
    expect(rows[1]?.textContent).toContain('Missing');
    expect(rows[2]?.textContent).toContain('Network stream');
    expect(root.querySelector('.fm-playlist-summary')?.textContent).toContain('3 tracks');
  });

  it('only mounts rows near the scroll window for long playlists', () => {
    const root = document.createElement('div');
    const tracks = Array.from({ length: 5000 }, (_, i) => ({
      title: `T${i}`,
      durationSeconds: undefined,
      target: `t${i}.mp3`,
      status: 'found' as const,
      location: undefined,
    }));
    m.render(root, m(PlaylistView, { content: { ...content, tracks } }));
    expect(root.querySelectorAll('.fm-playlist-row').length).toBeLessThan(100);
  });
});

describe('PlaylistView playback', () => {
  it('queues playable rows and starts from the double-clicked row', () => {
    const root = document.createElement('div');
    const onPlay = vi.fn();
    m.render(root, m(PlaylistView, { content, onPlay }));

    (root.querySelectorAll('.fm-playlist-action')[0] as HTMLButtonElement).click();
    const [items, startId, mode, skipped] = onPlay.mock.calls[0] ?? [];
    expect(items).toHaveLength(1);
    expect(startId).toBeUndefined();
    expect(mode).toBe('play');
    expect(skipped).toEqual({ remote: 1, missing: 1 });

    (root.querySelectorAll('.fm-playlist-row')[0] as HTMLElement).dispatchEvent(
      new MouseEvent('dblclick'),
    );
    const [rowItems, rowStart] = onPlay.mock.calls[1] ?? [];
    expect(rowStart).toBe(rowItems[0].id);
  });

  it('hides the play buttons when no player is attached', () => {
    const root = document.createElement('div');
    m.render(root, m(PlaylistView, { content }));
    expect(root.querySelector('.fm-playlist-action')).toBeNull();
  });
});
