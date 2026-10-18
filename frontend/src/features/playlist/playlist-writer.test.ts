import { describe, expect, it } from 'vitest';
import { parsePlaylist } from './playlist-parser';
import { initialPlaylistRows } from './playlist-resolver';
import { type PlaylistExportTrack, writePlaylist } from './playlist-writer';

const directory = { providerId: 'local', uri: 'file:///Users/me/music/mixes' };
const playlistLocation = { providerId: 'local', uri: 'file:///Users/me/music/mixes/out.m3u8' };

function track(uri: string, title?: string, durationSeconds?: number): PlaylistExportTrack {
  return { location: { providerId: 'local', uri }, title, durationSeconds };
}

const tracks = [
  track('file:///Users/me/music/mixes/a%20b.mp3', 'First, with comma', 215),
  track('file:///Users/me/music/albums/%E6%97%A5%E6%9C%AC.flac', undefined, undefined),
  track('file:///Volumes/usb/z.mp3', 'Far away', 61),
];

describe('writePlaylist', () => {
  it('writes M3U8 with #EXTM3U, EXTINF lines and relative paths', () => {
    const written = writePlaylist('m3u8', tracks, directory, 'relative');
    expect(written.pathStyle).toBe('relative');
    expect(written.content.split('\n')).toEqual([
      '#EXTM3U',
      '#EXTINF:215,First, with comma',
      'a b.mp3',
      '#EXTINF:-1,日本',
      '../albums/日本.flac',
      '#EXTINF:61,Far away',
      '../../../../Volumes/usb/z.mp3',
      '',
    ]);
  });

  it('writes native absolute paths for local files', () => {
    const written = writePlaylist('m3u8', tracks, directory, 'absolute');
    expect(written.content).toContain('\n/Users/me/music/mixes/a b.mp3\n');
    expect(written.content).toContain('\n/Volumes/usb/z.mp3\n');
    expect(written.fellBackToAbsolute).toBe(false);
  });

  it('falls back to absolute paths when tracks do not share the playlist provider root', () => {
    const mixed = [
      ...tracks,
      {
        location: { providerId: 'sftp', uri: 'sftp://host/music/x.mp3' },
        title: 'Remote',
        durationSeconds: 5,
      },
    ];
    const written = writePlaylist('m3u8', mixed, directory, 'relative');
    expect(written.pathStyle).toBe('absolute');
    expect(written.fellBackToAbsolute).toBe(true);
    expect(written.content).toContain('\nsftp://host/music/x.mp3\n');
  });

  it('writes valid PLS and XSPF', () => {
    const pls = writePlaylist('pls', tracks, directory, 'relative').content;
    expect(
      pls.startsWith('[playlist]\nFile1=a b.mp3\nTitle1=First, with comma\nLength1=215\n'),
    ).toBe(true);
    expect(pls).toContain('NumberOfEntries=3');
    const xspf = writePlaylist('xspf', tracks, directory, 'relative').content;
    expect(xspf).toContain('<location>a%20b.mp3</location>');
    expect(xspf).toContain('<duration>215000</duration>');
    expect(
      new DOMParser().parseFromString(xspf, 'application/xml').getElementsByTagName('parsererror'),
    ).toHaveLength(0);
  });

  it('escapes XML and keeps titles on one line', () => {
    const odd = [track('file:///Users/me/music/mixes/a.mp3', 'Fish & <Chips>\nline two')];
    const xspf = writePlaylist('xspf', odd, directory, 'relative').content;
    expect(xspf).toContain('<title>Fish &amp; &lt;Chips&gt; line two</title>');
    expect(writePlaylist('m3u8', odd, directory, 'relative').content).toContain(
      '#EXTINF:-1,Fish & <Chips> line two\n',
    );
  });
});

describe('round trip', () => {
  const cases = ['m3u8', 'pls', 'xspf'] as const;
  for (const format of cases) {
    for (const style of ['relative', 'absolute'] as const) {
      it(`${format} with ${style} paths parses back to the same entries`, () => {
        const { content } = writePlaylist(format, tracks, directory, style);
        const parsed = parsePlaylist(content, format);
        expect(parsed.error).toBeUndefined();
        expect(parsed.tracks).toHaveLength(tracks.length);
        const rows = initialPlaylistRows(playlistLocation, parsed.tracks);
        expect(rows.map((row) => row.location?.uri)).toEqual(tracks.map((t) => t.location.uri));
        expect(parsed.tracks.map((t) => t.durationSeconds)).toEqual([215, undefined, 61]);
        expect(parsed.tracks[0]?.title).toBe('First, with comma');
      });
    }
  }
});
