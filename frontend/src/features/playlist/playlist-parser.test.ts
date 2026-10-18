import { describe, expect, it } from 'vitest';
import {
  decodePlaylistBytes,
  MAX_PLAYLIST_TRACKS,
  parsePlaylist,
  playlistFormatForExtension,
} from './playlist-parser';

describe('playlistFormatForExtension', () => {
  it('maps the four playlist extensions case-insensitively', () => {
    expect(playlistFormatForExtension('M3U8')).toBe('m3u8');
    expect(playlistFormatForExtension('pls')).toBe('pls');
    expect(playlistFormatForExtension('xspf')).toBe('xspf');
    expect(playlistFormatForExtension('txt')).toBeUndefined();
    expect(playlistFormatForExtension(undefined)).toBeUndefined();
  });
});

describe('parsePlaylist m3u', () => {
  it('reads EXTINF titles and durations, applying them to the next entry only', () => {
    const parsed = parsePlaylist(
      '#EXTM3U\r\n#EXTINF:215,Artist - Song\r\nmusic/song.mp3\r\nplain.mp3\r\n#EXTINF:-1,Stream\r\nhttp://x.test/s',
      'm3u',
    );
    expect(parsed.tracks).toEqual([
      { title: 'Artist - Song', durationSeconds: 215, target: 'music/song.mp3' },
      { title: undefined, durationSeconds: undefined, target: 'plain.mp3' },
      { title: 'Stream', durationSeconds: undefined, target: 'http://x.test/s' },
    ]);
    expect(parsed.error).toBeUndefined();
  });

  it('keeps commas in titles and tolerates EXTINF attributes and a BOM', () => {
    const parsed = parsePlaylist('﻿#EXTINF:12 tvg-id="a",One, Two\nfile.mp3', 'm3u8');
    expect(parsed.tracks[0]).toEqual({
      title: 'One, Two',
      durationSeconds: 12,
      target: 'file.mp3',
    });
  });

  it('ignores comments and blank lines and survives garbage', () => {
    const parsed = parsePlaylist('# comment\n\n#EXTINF:oops\n   \n#EXTINF\n', 'm3u');
    expect(parsed.tracks).toEqual([]);
  });

  it('caps the track count', () => {
    const text = Array.from({ length: MAX_PLAYLIST_TRACKS + 5 }, (_, i) => `t${i}.mp3`).join('\n');
    const parsed = parsePlaylist(text, 'm3u');
    expect(parsed.tracks).toHaveLength(MAX_PLAYLIST_TRACKS);
    expect(parsed.truncated).toBe(true);
  });
});

describe('parsePlaylist pls', () => {
  it('orders entries by index and pairs titles and lengths', () => {
    const parsed = parsePlaylist(
      '[playlist]\nNumberOfEntries=2\nFile2=b.mp3\nTitle2=Beta\nLength2=61\nFile1=a.mp3\nLength1=-1\nVersion=2',
      'pls',
    );
    expect(parsed.tracks).toEqual([
      { title: undefined, durationSeconds: undefined, target: 'a.mp3' },
      { title: 'Beta', durationSeconds: 61, target: 'b.mp3' },
    ]);
  });

  it('reports a file without a playlist section', () => {
    expect(parsePlaylist('hello', 'pls').error).toBeDefined();
  });
});

describe('parsePlaylist xspf', () => {
  it('reads location, title and millisecond durations', () => {
    const parsed = parsePlaylist(
      `<?xml version="1.0"?><playlist version="1" xmlns="http://xspf.org/ns/0/"><trackList>
        <track><location>file:///music/a%20b.mp3</location><title>A B</title><duration>185500</duration></track>
        <track><title>No location</title></track>
        <track><location>http://x.test/s.mp3</location></track>
      </trackList></playlist>`,
      'xspf',
    );
    expect(parsed.tracks).toEqual([
      { title: 'A B', durationSeconds: 186, target: 'file:///music/a%20b.mp3' },
      { title: undefined, durationSeconds: undefined, target: 'http://x.test/s.mp3' },
    ]);
  });

  it('reports malformed XML without throwing', () => {
    const parsed = parsePlaylist('<playlist><trackList><track>', 'xspf');
    expect(parsed.tracks).toEqual([]);
    expect(parsed.error).toBeDefined();
  });
});

describe('decodePlaylistBytes', () => {
  it('decodes UTF-8 and falls back to Latin-1 only for legacy m3u', () => {
    const latin1 = new Uint8Array([0x63, 0x61, 0x66, 0xe9]); // "café" in Latin-1
    expect(decodePlaylistBytes(latin1, 'm3u')).toBe('café');
    expect(decodePlaylistBytes(latin1, 'm3u8')).toBe('caf�');
    expect(decodePlaylistBytes(new TextEncoder().encode('café'), 'm3u')).toBe('café');
  });

  it('honours a UTF-16LE byte order mark', () => {
    const bytes = new Uint8Array([0xff, 0xfe, 0x61, 0x00, 0x62, 0x00]);
    expect(decodePlaylistBytes(bytes, 'm3u')).toBe('﻿ab'.slice(1));
  });
});
