import m from 'mithril';
import { describe, expect, it } from 'vitest';
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
