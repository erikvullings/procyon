import m from 'mithril';

import type { FileManagerClient } from '../../api/client/file-manager-client';
import type { EntrySummary } from '../../models';
import { isParentEntry } from '../panes/parent-entry';

export type ThumbnailSize = 'small' | 'medium' | 'large';

type ThumbnailClient = Pick<FileManagerClient, 'getThumbnail' | 'readFileRange'>;

/** Extensions the backend can generate a preview for (task 0134): plain images
 * (including `ico`) directly, CBZ/CBR comic archives via their first page,
 * MP4/MOV/M4V video via its first H.264 keyframe, and PDF via a first-page
 * embedded image (not a real page render - see `fm_metadata::pdf`'s module
 * docs for that tradeoff). Kept in sync with
 * `fm_metadata::SUPPORTED_IMAGE_EXTENSIONS`/`SUPPORTED_VIDEO_EXTENSIONS`/
 * `SUPPORTED_PDF_EXTENSIONS` plus the cbz/cbr special case. `svg` is handled
 * separately below - it never goes through the (JPEG-only) thumbnail endpoint
 * since the browser already renders it natively. */
const THUMBNAILABLE_EXTENSIONS = new Set([
  'jpg',
  'jpeg',
  'png',
  'gif',
  'webp',
  'ico',
  'cbz',
  'cbr',
  'mp4',
  'm4v',
  'mov',
  'pdf',
  'svg',
]);

/** Large images are not useful as 16px list-row previews, but grid tiles must request them:
 * opening a cloud-backed placeholder is what lets OneDrive produce the visible thumbnail. */
const LIST_THUMBNAIL_SIZE_LIMIT_BYTES = 3 * 1024;
const SVG_THUMBNAIL_SIZE_LIMIT_BYTES = 512 * 1024;

const SIZE_GATED_EXTENSIONS = new Set(['jpg', 'jpeg', 'png', 'gif', 'webp', 'svg', 'pdf']);

function exceedsThumbnailSizeLimit(entry: EntrySummary, size: ThumbnailSize): boolean {
  if (size !== 'small') return false;
  const extension = (entry.extension ?? '').toLocaleLowerCase();
  if (!SIZE_GATED_EXTENSIONS.has(extension)) return false;
  return entry.size !== undefined && entry.size > LIST_THUMBNAIL_SIZE_LIMIT_BYTES;
}

function isThumbnailable(entry: EntrySummary, size: ThumbnailSize): boolean {
  if (entry.kind !== 'file') return false;
  if (isParentEntry(entry.id)) return false;
  if (!THUMBNAILABLE_EXTENSIONS.has((entry.extension ?? '').toLocaleLowerCase())) return false;
  return !exceedsThumbnailSizeLimit(entry, size);
}

function isSvg(entry: EntrySummary): boolean {
  return (entry.extension ?? '').toLocaleLowerCase() === 'svg';
}

function cacheKey(entry: EntrySummary, size: ThumbnailSize): string {
  // An SVG's rendering doesn't depend on the requested tile size (the browser scales the vector
  // markup itself), so it's fetched and cached once per file rather than once per size.
  return isSvg(entry) ? entry.location.uri : `${entry.location.uri}:${size}`;
}

function bytesToDataUri(bytes: Uint8Array, mimeType: string): string {
  let binary = '';
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return `data:${mimeType};base64,${btoa(binary)}`;
}

/** Lazily resolves and caches thumbnails without delaying directory row/tile
 * rendering (task 0134) - mirrors {@link NativeIconLoader}'s lazy/dedup/cache
 * shape, keyed per entry+size rather than per extension since a thumbnail is
 * specific to one file's content, not shared across every file of a type. */
/** Caps how many thumbnail/`readFileRange` requests are in flight at once. A directory full of
 * many thumbnailable files (e.g. a folder of SVGs) previously fired one request per visible tile
 * in the same render pass, with only same-key dedup - flooding the server and tripping its rate
 * limiter (429 Too Many Requests) well before any of them completed. */
const MAX_CONCURRENT_REQUESTS = 4;

interface PendingThumbnail {
  readonly key: string;
  readonly entry: EntrySummary;
  readonly size: ThumbnailSize;
  readonly controller: AbortController;
  readonly subscribers: Set<symbol>;
  state: 'queued' | 'active';
}

const UNSCOPED_SUBSCRIBER = Symbol('unscoped-thumbnail-request');

export class ThumbnailLoader {
  private readonly thumbnails = new Map<string, string | undefined>();
  private readonly pending = new Map<string, PendingThumbnail>();
  private activeRequestCount = 0;
  private readonly waiting: PendingThumbnail[] = [];

  constructor(
    private readonly client: ThumbnailClient,
    private readonly redraw: () => void = m.redraw,
  ) {}

  thumbnailDataUri(entry: EntrySummary, size: ThumbnailSize): string | undefined {
    return this.thumbnailDataUriFor(entry, size, UNSCOPED_SUBSCRIBER);
  }

  createViewport(): ThumbnailViewport {
    return new ThumbnailViewport(this);
  }

  thumbnailDataUriFor(
    entry: EntrySummary,
    size: ThumbnailSize,
    subscriber: symbol,
  ): string | undefined {
    if (!isThumbnailable(entry, size)) return undefined;
    const key = cacheKey(entry, size);
    if (this.thumbnails.has(key)) return this.thumbnails.get(key);
    const existing = this.pending.get(key);
    if (existing !== undefined && !existing.controller.signal.aborted) {
      existing.subscribers.add(subscriber);
      return undefined;
    }

    const request: PendingThumbnail = {
      key,
      entry,
      size,
      controller: new AbortController(),
      subscribers: new Set([subscriber]),
      state: this.activeRequestCount < MAX_CONCURRENT_REQUESTS ? 'active' : 'queued',
    };
    this.pending.set(key, request);
    if (request.state === 'active') {
      this.startRequest(request);
    } else {
      this.waiting.push(request);
    }
    return undefined;
  }

  unsubscribe(key: string, subscriber: symbol): void {
    const request = this.pending.get(key);
    if (request === undefined) return;
    request.subscribers.delete(subscriber);
    if (request.subscribers.size > 0) return;

    request.controller.abort();
    if (request.state === 'queued') {
      this.pending.delete(key);
    }
  }

  private startRequest(request: PendingThumbnail): void {
    this.activeRequestCount += 1;
    const signal = request.controller.signal;
    const result = isSvg(request.entry)
      ? this.readSvgDataUri(request.entry, signal)
      : this.client
          .getThumbnail(request.entry.location.uri, request.size, signal)
          .then((bytes) => (bytes === undefined ? undefined : bytesToDataUri(bytes, 'image/jpeg')));
    void result
      .then((dataUri) => {
        if (!signal.aborted) this.thumbnails.set(request.key, dataUri);
      })
      .catch(() => {
        if (!signal.aborted) this.thumbnails.set(request.key, undefined);
      })
      .finally(() => {
        this.activeRequestCount -= 1;
        if (this.pending.get(request.key) === request) this.pending.delete(request.key);
        this.startWaitingRequests();
        this.redraw();
      });
  }

  private startWaitingRequests(): void {
    while (this.activeRequestCount < MAX_CONCURRENT_REQUESTS) {
      const request = this.waiting.shift();
      if (request === undefined) return;
      if (request.controller.signal.aborted || this.pending.get(request.key) !== request) continue;
      request.state = 'active';
      this.startRequest(request);
    }
  }

  /** SVGs render natively in the browser, so this reads the raw markup directly rather than
   * routing through the (JPEG-only) thumbnail endpoint - no server-side rasterization needed. */
  private async readSvgDataUri(
    entry: EntrySummary,
    signal: AbortSignal,
  ): Promise<string | undefined> {
    if (entry.size !== undefined && entry.size > SVG_THUMBNAIL_SIZE_LIMIT_BYTES) return undefined;
    const chunk = await this.client.readFileRange(
      {
        location: entry.location,
        offset: 0,
        length: SVG_THUMBNAIL_SIZE_LIMIT_BYTES,
      },
      signal,
    );
    if (!chunk.eof) return undefined;
    return bytesToDataUri(Uint8Array.from(chunk.data), 'image/svg+xml');
  }
}

/** Tracks one grid's rendered tiles so requests that scroll out of its viewport can be cancelled. */
export class ThumbnailViewport {
  private readonly subscriber = Symbol('thumbnail-viewport');
  private previousKeys = new Set<string>();
  private currentKeys = new Set<string>();

  constructor(private readonly loader: ThumbnailLoader) {}

  beginFrame(): void {
    this.currentKeys.clear();
  }

  thumbnailDataUri(entry: EntrySummary, size: ThumbnailSize): string | undefined {
    if (isThumbnailable(entry, size)) this.currentKeys.add(cacheKey(entry, size));
    return this.loader.thumbnailDataUriFor(entry, size, this.subscriber);
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
