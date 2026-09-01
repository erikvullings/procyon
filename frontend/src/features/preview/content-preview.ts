import type { FileManagerClient } from '../../api/client/file-manager-client';
import type { EntrySummary } from '../../models';
import {
  AUDIO_EXTENSIONS,
  IMAGE_EXTENSIONS,
  VIDEO_EXTENSIONS,
} from '../directory-table/entry-icons';
import { ARCHIVE_SUFFIXES } from '../navigation/archive-location';

/**
 * Which inline preview renderer applies to an entry (task 0071's renderer registry, shared by
 * the cursor-driven preview panel and the Lister-style large-file viewer, task 0088). Extension
 * alone never proves an entry is safely textual - callers must also check the fetched chunk's
 * `probablyBinary` flag before rendering "text" content.
 */
export type PreviewKind =
  | 'text'
  | 'image'
  | 'audio'
  | 'video'
  | 'pdf'
  | 'comic'
  | 'epub'
  | 'docx'
  | 'pptx'
  | 'archiveSummary'
  | 'metadata'
  | 'unsupported';

/** Comic book archive extensions (zip/rar containers of page images) rendered page-by-page by the
 * F3 viewer's comic renderer, matching the archive-navigation extension list
 * (`frontend/src/features/navigation/archive-location.ts`). */
export const COMIC_ARCHIVE_EXTENSIONS = ['cbz', 'cbr'];

/**
 * Above this size, the lightweight cursor-driven preview panel shows a "too large to preview"
 * state instead of fetching content (task 0071's configurable-size-limit AC). The Lister viewer
 * (task 0088) has no such limit since it never loads more than its visible window. Fast-follow:
 * expose this as a user setting instead of a fixed constant.
 */
export const PREVIEW_SIZE_LIMIT_BYTES = 2 * 1024 * 1024;

/** Bytes fetched for a text preview snippet - enough for a few dozen lines without over-fetching. */
export const TEXT_PREVIEW_BYTES = 8 * 1024;

/** Resolves the preview/viewer renderer kind for `entry` from its kind and extension. */
export function resolvePreviewKind(entry: EntrySummary): PreviewKind {
  if (entry.kind !== 'file') {
    return 'metadata';
  }
  const extension = entry.extension?.toLowerCase();
  if (extension !== undefined && IMAGE_EXTENSIONS.includes(extension)) {
    return 'image';
  }
  if (extension !== undefined && AUDIO_EXTENSIONS.includes(extension)) {
    return 'audio';
  }
  if (extension !== undefined && VIDEO_EXTENSIONS.includes(extension)) {
    return 'video';
  }
  if (extension === 'pdf') {
    return 'pdf';
  }
  if (extension !== undefined && COMIC_ARCHIVE_EXTENSIONS.includes(extension)) {
    return 'comic';
  }
  if (extension === 'epub') {
    return 'epub';
  }
  if (extension === 'docx') {
    return 'docx';
  }
  if (extension === 'pptx') {
    return 'pptx';
  }
  const lowerName = entry.name.toLocaleLowerCase('en-US');
  if (ARCHIVE_SUFFIXES.some((suffix) => lowerName.endsWith(suffix))) {
    return 'archiveSummary';
  }
  return 'text';
}

const IMAGE_MIME_TYPES_BY_EXTENSION: Readonly<Record<string, string>> = {
  png: 'image/png',
  jpg: 'image/jpeg',
  jpeg: 'image/jpeg',
  gif: 'image/gif',
  webp: 'image/webp',
  bmp: 'image/bmp',
  svg: 'image/svg+xml',
  avif: 'image/avif',
  ico: 'image/x-icon',
};

/** The MIME type to use for an `<img>` data URI, preferring the extension over a stale/generic
 * server-reported `mimeType`. */
export function imageMimeTypeFor(entry: EntrySummary): string {
  const extension = entry.extension?.toLowerCase();
  const byExtension =
    extension === undefined ? undefined : IMAGE_MIME_TYPES_BY_EXTENSION[extension];
  return byExtension ?? entry.mimeType ?? 'application/octet-stream';
}

const AUDIO_MIME_TYPES_BY_EXTENSION: Readonly<Record<string, string>> = {
  mp3: 'audio/mpeg',
  wav: 'audio/wav',
  flac: 'audio/flac',
  ogg: 'audio/ogg',
  m4a: 'audio/mp4',
  aac: 'audio/aac',
};

/** The MIME type to use for an `<audio>` data URI, preferring the extension over a stale/generic
 * server-reported `mimeType`. */
export function audioMimeTypeFor(entry: EntrySummary): string {
  const extension = entry.extension?.toLowerCase();
  const byExtension =
    extension === undefined ? undefined : AUDIO_MIME_TYPES_BY_EXTENSION[extension];
  return byExtension ?? entry.mimeType ?? 'application/octet-stream';
}

const VIDEO_MIME_TYPES_BY_EXTENSION: Readonly<Record<string, string>> = {
  mp4: 'video/mp4',
  m4v: 'video/mp4',
  mov: 'video/quicktime',
  webm: 'video/webm',
  avi: 'video/x-msvideo',
};

/** The MIME type to use for a `<video>` data URI. MKV is intentionally omitted because browser
 * support depends on the codecs in the container, so it is routed to external playback instead. */
export function videoMimeTypeFor(entry: EntrySummary): string {
  const extension = entry.extension?.toLowerCase();
  const byExtension =
    extension === undefined ? undefined : VIDEO_MIME_TYPES_BY_EXTENSION[extension];
  return byExtension ?? entry.mimeType ?? 'application/octet-stream';
}

/**
 * Encodes raw bytes as a `data:` URI, the same approach `NativeIconLoader` uses for native icons
 * - avoids `URL.createObjectURL`, which jsdom does not implement, and needs no explicit revoke.
 */
export function bytesToDataUri(bytes: Uint8Array, mimeType: string): string {
  let binary = '';
  for (const byte of bytes) {
    binary += String.fromCharCode(byte);
  }
  return `data:${mimeType};base64,${btoa(binary)}`;
}

/** Bytes fetched per `readFileRange` call when reading a whole image/audio/small-video file - matches the
 * backend's `MAX_RANGE_LENGTH` cap so every request is served in one round trip when possible. */
export const IMAGE_RANGE_CHUNK_BYTES = 1024 * 1024;

/** Client surface required to read a file's full bytes in range-sized chunks. */
export type ImageRangeClient = Pick<FileManagerClient, 'readFileRange'>;

/**
 * Reads an entire file as range-sized chunks (respecting the backend's per-request byte cap).
 * Shared by the full image/audio readers and the size-gated video reader. The cursor-driven
 * preview panel never reaches this for entries over {@link PREVIEW_SIZE_LIMIT_BYTES}; the Lister
 * viewer keeps full images/audio unbounded per Total Commander convention, while its video caller
 * must enforce the separate inline threshold before invoking this function.
 */
export function readEntireFileBytes(
  client: ImageRangeClient,
  entry: EntrySummary,
  signal: AbortSignal,
): Promise<Uint8Array>;
export function readEntireFileBytes(
  client: ImageRangeClient,
  entry: EntrySummary,
  signal: AbortSignal,
  maximumBytes: number,
): Promise<Uint8Array | undefined>;
export async function readEntireFileBytes(
  client: ImageRangeClient,
  entry: EntrySummary,
  signal: AbortSignal,
  maximumBytes?: number,
): Promise<Uint8Array | undefined> {
  // Accumulate as typed-array segments rather than spreading each chunk's `number[]` into a
  // shared array (`chunks.push(...chunk.data)`) - for a 1 MiB chunk that spreads ~1,048,576
  // arguments into a single call and throws "Maximum call stack size exceeded".
  const segments: Uint8Array[] = [];
  let totalLength = 0;
  let offset = 0;
  for (;;) {
    const length =
      maximumBytes === undefined
        ? IMAGE_RANGE_CHUNK_BYTES
        : Math.min(IMAGE_RANGE_CHUNK_BYTES, maximumBytes + 1 - offset);
    if (length <= 0) return undefined;
    const chunk = await client.readFileRange({ location: entry.location, offset, length }, signal);
    const segment = Uint8Array.from(chunk.data);
    segments.push(segment);
    totalLength += segment.length;
    if (maximumBytes !== undefined && totalLength > maximumBytes) return undefined;
    offset += chunk.length;
    if (chunk.eof || chunk.length === 0) break;
  }
  const bytes = new Uint8Array(totalLength);
  let position = 0;
  for (const segment of segments) {
    bytes.set(segment, position);
    position += segment.length;
  }
  return bytes;
}

/** Reads an entire image file and encodes it as a `data:` URI (see {@link readEntireFileBytes}). */
export async function readFullImageDataUri(
  client: ImageRangeClient,
  entry: EntrySummary,
  signal: AbortSignal,
): Promise<string> {
  const bytes = await readEntireFileBytes(client, entry, signal);
  return bytesToDataUri(bytes, imageMimeTypeFor(entry));
}

/** Reads an entire audio file and encodes it as a `data:` URI (see {@link readEntireFileBytes}),
 * for the Lister viewer's native `<audio>` playback. */
export async function readFullAudioDataUri(
  client: ImageRangeClient,
  entry: EntrySummary,
  signal: AbortSignal,
): Promise<string> {
  const bytes = await readEntireFileBytes(client, entry, signal);
  return bytesToDataUri(bytes, audioMimeTypeFor(entry));
}

/** Reads an entire size-gated video file and encodes it as a `data:` URI. Callers must enforce
 * their video size limit before using this helper; unlike images/audio, videos are not unbounded. */
export async function readFullVideoDataUri(
  client: ImageRangeClient,
  entry: EntrySummary,
  signal: AbortSignal,
  maximumBytes: number,
): Promise<string | undefined> {
  const bytes = await readEntireFileBytes(client, entry, signal, maximumBytes);
  return bytes === undefined ? undefined : bytesToDataUri(bytes, videoMimeTypeFor(entry));
}
