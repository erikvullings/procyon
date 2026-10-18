import { describe, expect, it } from 'vitest';
import { formatTrackDuration, trackDisplayPath, trackDisplayTitle } from './playlist-format';

const base = { durationSeconds: undefined, status: 'found', location: undefined } as const;

describe('playlist formatting', () => {
  it('formats durations as m:ss and h:mm:ss', () => {
    expect(formatTrackDuration(undefined)).toBe('');
    expect(formatTrackDuration(5)).toBe('0:05');
    expect(formatTrackDuration(215)).toBe('3:35');
    expect(formatTrackDuration(3725)).toBe('1:02:05');
  });

  it('falls back to the file name when the title is missing', () => {
    expect(trackDisplayTitle({ ...base, title: undefined, target: '..\\x\\My%20Song.mp3' })).toBe(
      'My Song.mp3',
    );
    expect(trackDisplayTitle({ ...base, title: 'Given', target: 'a.mp3' })).toBe('Given');
  });

  it('shows the decoded resolved location', () => {
    expect(
      trackDisplayPath({
        ...base,
        title: undefined,
        target: 'a b.mp3',
        location: { providerId: 'local', uri: 'file:///m/a%20b.mp3' },
      }),
    ).toBe('file:///m/a b.mp3');
  });
});
