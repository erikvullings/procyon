import { describe, expect, it, vi } from 'vitest';
import type { EntrySummary, ImageGpsEntry, Location } from '../../models';
import { canWriteGps, GpsCache, isGeoCandidate, scanGps } from './gps-scan';

function entry(name: string, modifiedAt = '2024-01-01T00:00:00Z'): EntrySummary {
  return {
    id: name,
    name,
    kind: 'file',
    extension: name.split('.').pop(),
    modifiedAt,
    location: { providerId: 'local', uri: `file:///${name}` },
  } as EntrySummary;
}

function gpsFor(locations: readonly Location[]): ImageGpsEntry[] {
  return locations.map((location) =>
    location.uri.includes('geo')
      ? { location, gps: { latitude: 1, longitude: 2 }, takenAt: '2024-05-05T10:00:00' }
      : location.uri.includes('bad')
        ? { location, unreadable: true }
        : { location },
  );
}

describe('format helpers', () => {
  it('knows which files can carry and receive GPS data', () => {
    expect(isGeoCandidate(entry('a.JPG'))).toBe(true);
    expect(isGeoCandidate(entry('a.txt'))).toBe(false);
    expect(canWriteGps(entry('a.webp'))).toBe(true);
    expect(canWriteGps(entry('a.png'))).toBe(false);
  });
});

describe('scanGps', () => {
  it('splits located, unlocated and unreadable images and reports progress', async () => {
    const client = {
      readImageGps: vi.fn(async (locations: readonly Location[]) => gpsFor(locations)),
    };
    const progress = vi.fn();
    const result = await scanGps({
      client,
      entries: [entry('geo1.jpg'), entry('plain.jpg'), entry('bad.jpg')],
      cache: new GpsCache(),
      signal: new AbortController().signal,
      onProgress: progress,
    });
    expect(result.located.map((image) => image.entry.name)).toEqual(['geo1.jpg']);
    expect(result.located[0]).toMatchObject({
      latitude: 1,
      longitude: 2,
      takenAt: '2024-05-05T10:00:00',
    });
    expect(result.unlocated.map((e) => e.name)).toEqual(['plain.jpg']);
    expect(result.unreadable.map((e) => e.name)).toEqual(['bad.jpg']);
    expect(progress).toHaveBeenLastCalledWith(3, 3);
  });

  it('batches requests and reuses cached results by id and modified time', async () => {
    const client = {
      readImageGps: vi.fn(async (locations: readonly Location[]) => gpsFor(locations)),
    };
    const cache = new GpsCache();
    const entries = Array.from({ length: 60 }, (_, index) => entry(`geo${index}.jpg`));
    const signal = new AbortController().signal;
    await scanGps({ client, entries, cache, signal });
    expect(client.readImageGps).toHaveBeenCalledTimes(3);
    await scanGps({ client, entries, cache, signal });
    expect(client.readImageGps).toHaveBeenCalledTimes(3);
    const changed = entries.map((e, index) =>
      index === 0 ? entry(e.name, '2025-01-01T00:00:00Z') : e,
    );
    await scanGps({ client, entries: changed, cache, signal });
    expect(client.readImageGps).toHaveBeenCalledTimes(4);
  });

  it('stops when cancelled', async () => {
    const controller = new AbortController();
    const client = {
      readImageGps: vi.fn(async (locations: readonly Location[]) => {
        controller.abort();
        return gpsFor(locations);
      }),
    };
    const entries = Array.from({ length: 200 }, (_, index) => entry(`geo${index}.jpg`));
    await expect(
      scanGps({ client, entries, cache: new GpsCache(), signal: controller.signal }),
    ).rejects.toThrow();
    expect(client.readImageGps.mock.calls.length).toBeLessThan(8);
  });

  it('marks a batch unreadable when the request fails', async () => {
    const client = { readImageGps: vi.fn().mockRejectedValue(new Error('boom')) };
    const result = await scanGps({
      client,
      entries: [entry('a.jpg')],
      cache: new GpsCache(),
      signal: new AbortController().signal,
    });
    expect(result.unreadable).toHaveLength(1);
  });
});
