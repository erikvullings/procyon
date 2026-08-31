import { describe, expect, it } from 'vitest';
import type { Location } from '../../models';
import {
  MAX_RECENT_LOCATIONS,
  recordRecentLocation,
  reorderFavourites,
  truncateLocationForDisplay,
} from './favourites';

const location = (name: string): Location => ({ providerId: 'local', uri: `file:///tmp/${name}` });

describe('recordRecentLocation', () => {
  it('puts a visit first, deduplicates it, and bounds the list', () => {
    const first = location('first');
    const existing = Array.from({ length: MAX_RECENT_LOCATIONS }, (_, index) =>
      location(`${index}`),
    );

    expect(recordRecentLocation(existing, first).map((item) => item.uri)).toEqual([
      first.uri,
      ...existing
        .filter((item) => item.uri !== first.uri)
        .slice(0, MAX_RECENT_LOCATIONS - 1)
        .map((item) => item.uri),
    ]);
  });

  it('does not restore a location removed from favourites', () => {
    const removed = location('removed');
    const existing = [location('kept')];

    expect(recordRecentLocation(existing, removed, [removed])).toEqual(existing);
  });

  it('does not retain session-only search and archive locations', () => {
    const existing = [
      { providerId: 'search', uri: 'search://local/expired-search' },
      { providerId: 'archive', uri: 'archive:///tmp/book.zip!/' },
      location('kept'),
    ];

    expect(
      recordRecentLocation(existing, {
        providerId: 'search',
        uri: 'search://local/current-search',
      }),
    ).toEqual([location('kept')]);
  });
});

describe('reorderFavourites', () => {
  it('moves a favourite while preserving every other favourite order', () => {
    const favourites = ['One', 'Two', 'Three'].map((label) => ({
      label,
      location: location(label),
    }));
    expect(reorderFavourites(favourites, 2, 0).map((favourite) => favourite.label)).toEqual([
      'Three',
      'One',
      'Two',
    ]);
  });
});

describe('truncateLocationForDisplay', () => {
  it('returns the uri unchanged when it already fits', () => {
    expect(truncateLocationForDisplay('file:///tmp/short', 56)).toBe('file:///tmp/short');
  });

  it('cuts the middle of the path, keeping the scheme and the trailing segment', () => {
    const uri = `file:///Users/erik/dev/${'a'.repeat(60)}/reports/quarterly/summary.pdf`;
    const result = truncateLocationForDisplay(uri, 56);

    expect(result.length).toBe(56);
    expect(result.startsWith('file://')).toBe(true);
    expect(result.endsWith('summary.pdf')).toBe(true);
    expect(result).toContain('…');
  });

  it('preserves an archive:// scheme through truncation', () => {
    const uri = `archive:///${'a'.repeat(80)}/inner.txt`;
    const result = truncateLocationForDisplay(uri, 40);

    expect(result.startsWith('archive://')).toBe(true);
    expect(result.endsWith('inner.txt')).toBe(true);
  });
});
