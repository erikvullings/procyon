import type { FileManagerClient } from '../../api/client/file-manager-client';
import type { EntrySummary, ImageGpsEntry } from '../../models';

/** Images are sent to the backend in batches of this many (the backend accepts up to 100). */
const BATCH_SIZE = 25;
/** Batches in flight at once; kept low so remote providers are not flooded. */
const CONCURRENCY = 2;

const GEO_EXTENSIONS: ReadonlySet<string> = new Set([
  'jpg',
  'jpeg',
  'tif',
  'tiff',
  'webp',
  'png',
  'heic',
  'heif',
  'avif',
]);

function extensionOf(entry: EntrySummary): string {
  return (entry.extension ?? entry.name.split('.').pop() ?? '').toLowerCase();
}

/** Formats that can carry EXIF GPS tags. */
export function isGeoCandidate(entry: EntrySummary): boolean {
  return entry.kind === 'file' && GEO_EXTENSIONS.has(extensionOf(entry));
}

/** Formats the metadata editor can write coordinates into (mirrors the backend writer). */
export function canWriteGps(entry: EntrySummary): boolean {
  return (
    entry.kind === 'file' && ['jpg', 'jpeg', 'tif', 'tiff', 'webp'].includes(extensionOf(entry))
  );
}

export interface GeoImage {
  readonly entry: EntrySummary;
  readonly latitude: number;
  readonly longitude: number;
  readonly takenAt: string | undefined;
}

export interface GpsScanResult {
  readonly located: readonly GeoImage[];
  /** Images without GPS tags. */
  readonly unlocated: readonly EntrySummary[];
  /** Images that could not be read at all. */
  readonly unreadable: readonly EntrySummary[];
}

type CachedGps = Pick<ImageGpsEntry, 'gps' | 'takenAt' | 'unreadable'>;

/** Remembers scan results per entry id and modification time, so re-opening is instant. */
export class GpsCache {
  private readonly results = new Map<string, CachedGps>();

  private key(entry: EntrySummary): string {
    return `${entry.id}|${entry.modifiedAt ?? ''}|${entry.size ?? ''}`;
  }

  get(entry: EntrySummary): CachedGps | undefined {
    return this.results.get(this.key(entry));
  }

  set(entry: EntrySummary, value: CachedGps): void {
    this.results.set(this.key(entry), value);
  }

  /** Forgets one entry, for example after its coordinates were edited. */
  forget(entry: EntrySummary): void {
    this.results.delete(this.key(entry));
  }
}

export interface GpsScanOptions {
  readonly client: Pick<FileManagerClient, 'readImageGps'>;
  readonly entries: readonly EntrySummary[];
  readonly cache: GpsCache;
  readonly signal: AbortSignal;
  readonly onProgress?: (done: number, total: number) => void;
}

/**
 * Reads the GPS data of `entries` in bounded batches. Cancelling rejects with the abort reason;
 * entries already resolved stay in the cache.
 */
export async function scanGps(options: GpsScanOptions): Promise<GpsScanResult> {
  const { client, entries, cache, signal } = options;
  const pending = entries.filter((entry) => cache.get(entry) === undefined);
  let done = entries.length - pending.length;
  options.onProgress?.(done, entries.length);
  const batches: EntrySummary[][] = [];
  for (let start = 0; start < pending.length; start += BATCH_SIZE) {
    batches.push(pending.slice(start, start + BATCH_SIZE));
  }
  let next = 0;
  async function worker(): Promise<void> {
    while (next < batches.length) {
      signal.throwIfAborted();
      const batch = batches[next++];
      if (batch === undefined) return;
      let results: ImageGpsEntry[];
      try {
        results = await client.readImageGps(
          batch.map((entry) => entry.location),
          signal,
        );
      } catch {
        signal.throwIfAborted();
        results = batch.map((entry) => ({ location: entry.location, unreadable: true }));
      }
      batch.forEach((entry, index) => {
        const result = results[index];
        cache.set(entry, {
          gps: result?.gps ?? null,
          takenAt: result?.takenAt ?? null,
          unreadable: result === undefined || result.unreadable === true,
        });
      });
      done += batch.length;
      options.onProgress?.(done, entries.length);
    }
  }
  await Promise.all(Array.from({ length: Math.min(CONCURRENCY, batches.length) }, worker));
  signal.throwIfAborted();

  const located: GeoImage[] = [];
  const unlocated: EntrySummary[] = [];
  const unreadable: EntrySummary[] = [];
  for (const entry of entries) {
    const cached = cache.get(entry);
    if (cached?.unreadable === true) unreadable.push(entry);
    else if (cached?.gps != null) {
      located.push({
        entry,
        latitude: cached.gps.latitude,
        longitude: cached.gps.longitude,
        takenAt: cached.takenAt ?? undefined,
      });
    } else unlocated.push(entry);
  }
  return { located, unlocated, unreadable };
}
