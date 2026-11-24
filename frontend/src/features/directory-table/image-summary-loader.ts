import m from 'mithril';

import type { WorkspaceClient } from '../../api/client/workspace-client.ts';
import type { EntrySummary, ImageSummary } from '../../models';
import { WeightedLru } from '../../utilities/weighted-lru';
import { isParentEntry } from '../panes/parent-entry';

type SummaryClient = Pick<WorkspaceClient, 'readImageSummaries'>;

/** Formats the backend reads headers and EXIF from (SVG is vector markup, not a photo). */
const SUMMARY_EXTENSIONS = new Set([
  'jpg',
  'jpeg',
  'png',
  'gif',
  'webp',
  'tif',
  'tiff',
  'heic',
  'heif',
  'avif',
]);

/** The backend rejects larger batches (`MAX_SUMMARY_LOCATIONS`). */
const MAX_BATCH_SIZE = 64;
/** Requests made in the same render pass are collected for this long and sent as one batch. */
const BATCH_DELAY_MS = 50;
const MAX_CONCURRENT_BATCHES = 2;
/** Summaries are tiny; this only bounds a window that browses for days. */
const CACHE_ENTRIES = 5_000;

const UNSCOPED_SUBSCRIBER = Symbol('unscoped-image-summary-request');

interface Wanted {
  readonly entry: EntrySummary;
  readonly subscribers: Set<symbol>;
}

interface Batch {
  readonly keys: readonly string[];
  readonly controller: AbortController;
}

/** True for files whose picture metadata the backend can read. */
export function isImageEntry(entry: EntrySummary): boolean {
  return (
    entry.kind === 'file' &&
    !isParentEntry(entry.id) &&
    SUMMARY_EXTENSIONS.has((entry.extension ?? '').toLocaleLowerCase())
  );
}

/** A file read again after it changed, so the key carries its size and modification time. */
export function summaryKey(entry: EntrySummary): string {
  return `${entry.location.uri}|${entry.size ?? ''}|${entry.modifiedAt ?? ''}`;
}

/**
 * Lazily resolves picture summaries (size, camera, exposure, ...) for grid captions and tooltips
 * (task 0347), without delaying rendering. Entries asked for in the same pass are sent as one
 * batched, cancellable request; results are cached per file version, and a failure is cached as
 * "unavailable" so a broken provider is not retried on every render.
 */
export class ImageSummaryLoader {
  private readonly cache = new WeightedLru<ImageSummary>(CACHE_ENTRIES, () => 1);
  private readonly queued = new Map<string, Wanted>();
  private readonly inFlight = new Map<string, Wanted>();
  private readonly batches = new Set<Batch>();
  private timer: ReturnType<typeof setTimeout> | undefined;

  constructor(
    private readonly client: SummaryClient,
    private readonly redraw: () => void = m.redraw,
  ) {}

  createViewport(): ImageSummaryViewport {
    return new ImageSummaryViewport(this);
  }

  summaryFor(
    entry: EntrySummary,
    subscriber: symbol = UNSCOPED_SUBSCRIBER,
  ): ImageSummary | undefined {
    if (!isImageEntry(entry)) return undefined;
    const key = summaryKey(entry);
    if (this.cache.has(key)) return this.cache.get(key);
    const wanted = this.queued.get(key) ?? this.inFlight.get(key);
    if (wanted !== undefined) {
      wanted.subscribers.add(subscriber);
      return undefined;
    }
    this.queued.set(key, { entry, subscribers: new Set([subscriber]) });
    this.scheduleFlush();
    return undefined;
  }

  unsubscribe(key: string, subscriber: symbol): void {
    const queued = this.queued.get(key);
    if (queued !== undefined) {
      queued.subscribers.delete(subscriber);
      if (queued.subscribers.size === 0) this.queued.delete(key);
      return;
    }
    const flying = this.inFlight.get(key);
    if (flying === undefined) return;
    flying.subscribers.delete(subscriber);
    for (const batch of this.batches) {
      if (batch.keys.includes(key) && this.nobodyWaits(batch)) batch.controller.abort();
    }
  }

  private nobodyWaits(batch: Batch): boolean {
    return batch.keys.every((key) => (this.inFlight.get(key)?.subscribers.size ?? 0) === 0);
  }

  private scheduleFlush(): void {
    if (this.timer !== undefined) return;
    this.timer = setTimeout(() => {
      this.timer = undefined;
      this.flush();
    }, BATCH_DELAY_MS);
  }

  private flush(): void {
    if (this.batches.size >= MAX_CONCURRENT_BATCHES || this.queued.size === 0) return;
    const taken = [...this.queued.entries()].slice(0, MAX_BATCH_SIZE);
    for (const [key, wanted] of taken) {
      this.queued.delete(key);
      this.inFlight.set(key, wanted);
    }
    const batch: Batch = { keys: taken.map(([key]) => key), controller: new AbortController() };
    this.batches.add(batch);
    const locations = taken.map(([, wanted]) => wanted.entry.location);
    void this.client
      .readImageSummaries(locations, batch.controller.signal)
      .then((summaries) => {
        if (batch.controller.signal.aborted) return;
        taken.forEach(([key, wanted], index) => {
          this.cache.set(key, summaries[index] ?? unavailable(wanted.entry));
        });
      })
      .catch(() => {
        if (batch.controller.signal.aborted) return;
        for (const [key, wanted] of taken) this.cache.set(key, unavailable(wanted.entry));
      })
      .finally(() => {
        this.batches.delete(batch);
        for (const key of batch.keys) this.inFlight.delete(key);
        if (this.queued.size > 0) this.scheduleFlush();
        this.redraw();
      });
  }
}

function unavailable(entry: EntrySummary): ImageSummary {
  return { location: entry.location, unreadable: true };
}

/** Tracks one grid's rendered tiles so requests for tiles that scrolled away can be cancelled. */
export class ImageSummaryViewport {
  private readonly subscriber = Symbol('image-summary-viewport');
  private previousKeys = new Set<string>();
  private currentKeys = new Set<string>();

  constructor(private readonly loader: ImageSummaryLoader) {}

  beginFrame(): void {
    this.currentKeys.clear();
  }

  summaryFor(entry: EntrySummary): ImageSummary | undefined {
    if (isImageEntry(entry)) this.currentKeys.add(summaryKey(entry));
    return this.loader.summaryFor(entry, this.subscriber);
  }

  endFrame(): void {
    for (const key of this.previousKeys) {
      if (!this.currentKeys.has(key)) this.loader.unsubscribe(key, this.subscriber);
    }
    [this.previousKeys, this.currentKeys] = [this.currentKeys, this.previousKeys];
  }

  dispose(): void {
    for (const key of this.previousKeys) this.loader.unsubscribe(key, this.subscriber);
    for (const key of this.currentKeys) this.loader.unsubscribe(key, this.subscriber);
    this.previousKeys.clear();
    this.currentKeys.clear();
  }
}
