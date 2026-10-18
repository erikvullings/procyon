import m from 'mithril';
import { describe, expect, it, vi } from 'vitest';
import { playlistFileName, SavePlaylistDialog } from './save-playlist-dialog';

describe('playlistFileName', () => {
  it('appends the extension unless it is already present', () => {
    expect(playlistFileName('mix', 'm3u8')).toBe('mix.m3u8');
    expect(playlistFileName(' Mix.M3U8 ', 'm3u8')).toBe('Mix.M3U8');
    expect(playlistFileName('mix.m3u8', 'pls')).toBe('mix.m3u8.pls');
  });
});

describe('SavePlaylistDialog', () => {
  it('confirms with the default name, format and relative paths', () => {
    const root = document.createElement('div');
    document.body.append(root);
    const onConfirm = vi.fn();
    m.render(
      root,
      m(SavePlaylistDialog, {
        request: {
          tracks: [
            {
              location: { providerId: 'local', uri: 'file:///a.mp3' },
              title: 'A',
              durationSeconds: undefined,
            },
          ],
          directory: { providerId: 'local', uri: 'file:///Users/me/My%20Music' },
        },
        onConfirm,
        onCancel: vi.fn(),
      }),
    );

    expect(root.textContent).toContain('1 track will be saved.');
    expect(root.textContent).toContain('/Users/me/My Music');
    const save = Array.from(root.querySelectorAll('button')).find(
      (button) => button.textContent === 'Save',
    );
    save?.click();
    expect(onConfirm).toHaveBeenCalledWith('playlist.m3u8', 'm3u8', 'relative');
    root.remove();
  });
});
