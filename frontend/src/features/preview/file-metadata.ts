import { parse as parseExif } from 'exifr';
import type { EntrySummary } from '../../models';
import type { EditableLanguage } from '../editor/editor-language';

/** Technical metadata shown in the F3 viewer's Alt+Space info panel for an image entry. */
export interface FileViewerImageMetadata {
  readonly kind: 'image';
  readonly width: number | undefined;
  readonly height: number | undefined;
  readonly sizeBytes: number | undefined;
  readonly mimeType: string;
  readonly cameraMake: string | undefined;
  readonly cameraModel: string | undefined;
  readonly dateTaken: string | undefined;
  readonly gpsLatitude: number | undefined;
  readonly gpsLongitude: number | undefined;
}

/** Technical metadata shown in the F3 viewer's Alt+Space info panel for a text entry. Line/char
 * counts reflect only the currently loaded window (task 0088's windowed loading), not the whole
 * file, for files larger than one window. */
export interface FileViewerTextMetadata {
  readonly kind: 'text';
  readonly sizeBytes: number | undefined;
  readonly lineCount: number;
  readonly characterCount: number;
  readonly language: EditableLanguage;
  readonly windowedCount: boolean;
}

export type FileViewerMetadata = FileViewerImageMetadata | FileViewerTextMetadata;

function isDate(value: unknown): value is Date {
  return value instanceof Date && !Number.isNaN(value.getTime());
}

function isFiniteNumber(value: unknown): value is number {
  return typeof value === 'number' && Number.isFinite(value);
}

function isNonEmptyString(value: unknown): value is string {
  return typeof value === 'string' && value.trim() !== '';
}

/** Parses EXIF (camera make/model, date taken, GPS) from an image `data:` URI. Returns an object
 * with all fields `undefined` for images without EXIF (e.g. PNG, or JPEGs with stripped EXIF)
 * rather than throwing - EXIF is optional enrichment, never load-bearing. */
export async function readImageExif(
  dataUri: string,
): Promise<
  Pick<
    FileViewerImageMetadata,
    'cameraMake' | 'cameraModel' | 'dateTaken' | 'gpsLatitude' | 'gpsLongitude'
  >
> {
  try {
    const tags = (await parseExif(dataUri, { gps: true })) as Record<string, unknown> | undefined;
    return {
      cameraMake: isNonEmptyString(tags?.Make) ? tags.Make : undefined,
      cameraModel: isNonEmptyString(tags?.Model) ? tags.Model : undefined,
      dateTaken: isDate(tags?.DateTimeOriginal)
        ? tags.DateTimeOriginal.toISOString()
        : isDate(tags?.CreateDate)
          ? tags.CreateDate.toISOString()
          : undefined,
      gpsLatitude: isFiniteNumber(tags?.latitude) ? tags.latitude : undefined,
      gpsLongitude: isFiniteNumber(tags?.longitude) ? tags.longitude : undefined,
    };
  } catch {
    return {
      cameraMake: undefined,
      cameraModel: undefined,
      dateTaken: undefined,
      gpsLatitude: undefined,
      gpsLongitude: undefined,
    };
  }
}

/** Decodes an image `data:` URI just far enough to read its pixel dimensions, without keeping the
 * decoded bitmap around. Returns `undefined` when the environment can't decode images (e.g. the
 * jsdom test environment) rather than throwing. */
export async function readImageDimensions(
  dataUri: string,
): Promise<{ readonly width: number; readonly height: number } | undefined> {
  if (typeof createImageBitmap !== 'function' || typeof fetch !== 'function') return undefined;
  try {
    const blob = await (await fetch(dataUri)).blob();
    const bitmap = await createImageBitmap(blob);
    const dimensions = { width: bitmap.width, height: bitmap.height };
    bitmap.close();
    return dimensions;
  } catch {
    return undefined;
  }
}

/** Builds the info-panel metadata for a text entry from its currently loaded window - synchronous
 * and free, unlike the image path, since no extra parsing/decoding is needed. */
export function textMetadataFor(
  entry: EntrySummary,
  text: string,
  windowedCount: boolean,
  language: EditableLanguage,
): FileViewerTextMetadata {
  return {
    kind: 'text',
    sizeBytes: entry.size,
    lineCount: text.length === 0 ? 0 : text.split('\n').length,
    characterCount: text.length,
    language,
    windowedCount,
  };
}

/** A Google Maps link for a GPS coordinate pair, for the info panel's clickable location field. */
export function mapLinkFor(latitude: number, longitude: number): string {
  return `https://www.google.com/maps?q=${latitude},${longitude}`;
}
