import { t } from '../../i18n';
import type { EntrySummary, ImageSummary } from '../../models';
import {
  DEFAULT_ENTRY_FORMAT_SETTINGS,
  type EntryFormatSettings,
  formatEntryModifiedAt,
  formatEntrySize,
} from '../entry-formatting/entry-formatting';
import type { ThumbnailSize } from './thumbnail-loader';

/** Everything a field needs to render itself; `summary` is absent until the metadata arrived. */
interface FieldContext {
  readonly entry: EntrySummary;
  readonly summary: ImageSummary | undefined;
  readonly settings: EntryFormatSettings;
}

/** Fields in the registry; captions and tooltips each show a chosen subset of these (task 0347). */
export type ImageFieldId =
  | 'size'
  | 'dimensions'
  | 'modified'
  | 'dateTaken'
  | 'camera'
  | 'exposure'
  | 'aperture'
  | 'shutterSpeed'
  | 'iso'
  | 'exposureBias'
  | 'focalLength'
  | 'whiteBalance'
  | 'flash'
  | 'rating'
  | 'title'
  | 'artist'
  | 'album'
  | 'genre'
  | 'year'
  | 'track'
  | 'duration'
  | 'bitrate'
  | 'sampleRate'
  | 'audioFormat'
  | 'frameRate'
  | 'videoCodec';

/** A field renders to its value, or `undefined` when the file has none, so nothing blank shows. */
type FieldFormatter = (context: FieldContext) => string | undefined;

function dimensions({ summary }: FieldContext): string | undefined {
  const width = summary?.width;
  const height = summary?.height;
  if (width == null || height == null) return undefined;
  const depth = summary?.bitDepth;
  return depth == null ? `${width} × ${height}` : `${width} × ${height} × ${depth}`;
}

function number(value: number, { locale }: EntryFormatSettings): string {
  return new Intl.NumberFormat(locale, { maximumFractionDigits: 1 }).format(value);
}

/** Exposures of a second or more read as seconds, shorter ones as the usual `1/250` fraction. */
function shutter(seconds: number, settings: EntryFormatSettings): string {
  return seconds >= 1 ? `${number(seconds, settings)} s` : `1/${Math.round(1 / seconds)} s`;
}

function exposure(context: FieldContext): string | undefined {
  const iso = context.summary?.iso;
  const parts = [
    FORMATTERS.aperture(context),
    FORMATTERS.shutterSpeed(context),
    iso == null ? undefined : `ISO ${iso}`,
  ].filter((part): part is string => part !== undefined);
  return parts.length === 0 ? undefined : parts.join(' · ');
}

const WALL_CLOCK = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})/;

/** A capture time is the camera's wall clock without a zone: show it as recorded, never shifted
 * into the configured time zone. */
function wallClock(takenAt: string, settings: EntryFormatSettings): string | undefined {
  const match = WALL_CLOCK.exec(takenAt);
  if (match === null) return undefined;
  const [year, month, day, hour, minute, second] = match.slice(1).map(Number) as [
    number,
    number,
    number,
    number,
    number,
    number,
  ];
  const instant = new Date(Date.UTC(year, month - 1, day, hour, minute, second));
  return formatEntryModifiedAt(instant.toISOString(), { ...settings, timeZone: 'UTC' });
}

const SECONDS_PER_MINUTE = 60;
const SECONDS_PER_HOUR = 3_600;

/** Playing time as `m:ss`, or `h:mm:ss` from one hour on. */
function playingTime(totalSeconds: number): string {
  const seconds = Math.round(totalSeconds);
  const hours = Math.floor(seconds / SECONDS_PER_HOUR);
  const minutes = Math.floor((seconds % SECONDS_PER_HOUR) / SECONDS_PER_MINUTE);
  const rest = String(seconds % SECONDS_PER_MINUTE).padStart(2, '0');
  return hours > 0 ? `${hours}:${String(minutes).padStart(2, '0')}:${rest}` : `${minutes}:${rest}`;
}

const MAX_RATING = 5;

function rating({ summary }: FieldContext): string | undefined {
  const stars = summary?.rating;
  if (stars == null || stars <= 0) return undefined;
  const filled = Math.min(stars, MAX_RATING);
  return '★'.repeat(filled) + '☆'.repeat(MAX_RATING - filled);
}

function signed(value: number, { locale }: EntryFormatSettings): string {
  return new Intl.NumberFormat(locale, {
    maximumFractionDigits: 1,
    signDisplay: 'exceptZero',
  }).format(value);
}

const FORMATTERS: Readonly<Record<ImageFieldId, FieldFormatter>> = {
  modified: ({ entry, settings }) =>
    entry.modifiedAt === undefined ? undefined : formatEntryModifiedAt(entry.modifiedAt, settings),
  aperture: ({ summary, settings }) =>
    summary?.aperture == null ? undefined : `f/${number(summary.aperture, settings)}`,
  shutterSpeed: ({ summary, settings }) =>
    summary?.shutterSeconds == null || summary.shutterSeconds <= 0
      ? undefined
      : shutter(summary.shutterSeconds, settings),
  iso: ({ summary }) => (summary?.iso == null ? undefined : String(summary.iso)),
  exposureBias: ({ summary, settings }) =>
    summary?.exposureBiasEv == null ? undefined : `${signed(summary.exposureBiasEv, settings)} EV`,
  focalLength: ({ summary, settings }) =>
    summary?.focalLengthMm == null ? undefined : `${number(summary.focalLengthMm, settings)} mm`,
  whiteBalance: ({ summary }) =>
    summary?.whiteBalanceAuto == null
      ? undefined
      : t('imageMeta', summary.whiteBalanceAuto ? 'whiteBalanceAuto' : 'whiteBalanceManual'),
  flash: ({ summary }) =>
    summary?.flashFired == null
      ? undefined
      : t('imageMeta', summary.flashFired ? 'flashFired' : 'flashOff'),
  size: ({ entry, settings }) =>
    entry.size === undefined ? undefined : formatEntrySize(entry, settings),
  dimensions,
  dateTaken: ({ summary, settings }) =>
    summary?.takenAt == null ? undefined : wallClock(summary.takenAt, settings),
  exposure,
  rating,
  camera: ({ summary }) => summary?.camera ?? undefined,
  title: ({ summary }) => summary?.title ?? undefined,
  artist: ({ summary }) => summary?.artist ?? undefined,
  album: ({ summary }) => summary?.album ?? undefined,
  genre: ({ summary }) => summary?.genre ?? undefined,
  year: ({ summary }) => (summary?.year == null ? undefined : String(summary.year)),
  track: ({ summary }) => (summary?.trackNumber == null ? undefined : String(summary.trackNumber)),
  duration: ({ summary }) =>
    summary?.durationSeconds == null || summary.durationSeconds <= 0
      ? undefined
      : playingTime(summary.durationSeconds),
  bitrate: ({ summary }) =>
    summary?.bitrateKbps == null ? undefined : `${summary.bitrateKbps} kbps`,
  sampleRate: ({ summary, settings }) =>
    summary?.sampleRateHz == null
      ? undefined
      : `${number(summary.sampleRateHz / 1000, settings)} kHz`,
  audioFormat: ({ summary }) => summary?.audioFormat?.toLocaleUpperCase() || undefined,
  frameRate: ({ summary, settings }) =>
    summary?.frameRate == null ? undefined : `${number(summary.frameRate, settings)} fps`,
  videoCodec: ({ summary }) => summary?.videoCodec ?? undefined,
};

/** The chosen fields the file has, in order; empty ones are dropped so nothing renders blank. */
function formattedFields(
  fields: readonly ImageFieldId[],
  context: FieldContext,
): { readonly id: ImageFieldId; readonly value: string }[] {
  return fields.flatMap((id) => {
    const value = FORMATTERS[id](context);
    return value === undefined || value === '' ? [] : [{ id, value }];
  });
}

/** The lines shown under a grid tile's name: one per chosen field, empty fields left out. */
export function captionLines(
  fields: readonly ImageFieldId[],
  entry: EntrySummary,
  summary: ImageSummary | undefined,
  settings: EntryFormatSettings = DEFAULT_ENTRY_FORMAT_SETTINGS,
): string[] {
  return formattedFields(fields, { entry, summary, settings }).map(({ value }) => value);
}

/** One row of the hover tooltip's two-column table. */
export interface TooltipRow {
  readonly id: ImageFieldId;
  readonly label: string;
  readonly value: string;
}

/** The tooltip table: a labelled row per chosen field the file actually has. */
export function tooltipRows(
  fields: readonly ImageFieldId[],
  entry: EntrySummary,
  summary: ImageSummary | undefined,
  settings: EntryFormatSettings = DEFAULT_ENTRY_FORMAT_SETTINGS,
): TooltipRow[] {
  const context: FieldContext = { entry, summary, settings };
  return fields.flatMap((id) => {
    const value = FORMATTERS[id](context);
    return value === undefined || value === '' ? [] : [{ id, label: t('imageMeta', id), value }];
  });
}

/** Fields a grid caption can show, in the order the settings editor lists them. */
export const CAPTION_FIELD_CHOICES: readonly ImageFieldId[] = [
  'size',
  'dimensions',
  'modified',
  'dateTaken',
  'camera',
  'exposure',
  'rating',
  'artist',
  'title',
  'album',
  'duration',
];

/** Fields the tooltip table can show, in display order. */
export const TOOLTIP_FIELD_CHOICES: readonly ImageFieldId[] = [
  'size',
  'dimensions',
  'modified',
  'dateTaken',
  'camera',
  'aperture',
  'shutterSpeed',
  'iso',
  'exposureBias',
  'focalLength',
  'whiteBalance',
  'flash',
  'rating',
  'title',
  'artist',
  'album',
  'genre',
  'year',
  'track',
  'duration',
  'bitrate',
  'sampleRate',
  'audioFormat',
  'frameRate',
  'videoCodec',
];

/** Keeps the ids this build knows, so a setting written by a newer version cannot break rendering. */
export function knownFields(ids: readonly string[]): ImageFieldId[] {
  return ids.filter((id): id is ImageFieldId => Object.hasOwn(FORMATTERS, id));
}

/** Fields the listing already knows, so they render without waiting for image metadata. */
const LISTING_FIELDS: ReadonlySet<ImageFieldId> = new Set<ImageFieldId>(['size', 'modified']);

/** True when rendering any of `fields` needs the metadata read from the image file. */
export function needsImageSummary(fields: readonly ImageFieldId[]): boolean {
  return fields.some((field) => !LISTING_FIELDS.has(field));
}

/** The caption fields a tile of `size` shows: the small size is too narrow for any. */
export function captionFieldsFor(
  size: ThumbnailSize,
  captionFields: readonly ImageFieldId[] | undefined,
): readonly ImageFieldId[] {
  return size === 'small' ? [] : (captionFields ?? []);
}

/** Height of one caption line under a grid tile, in CSS pixels. */
export const CAPTION_LINE_HEIGHT_PX = 14;
const CAPTION_BLOCK_GAP_PX = 6;

/** Extra tile height for `fieldCount` caption lines. Every tile reserves the same space, empty
 * lines included, so the virtualized grid keeps one uniform tile height. */
export function captionBlockHeight(fieldCount: number): number {
  return fieldCount === 0 ? 0 : CAPTION_BLOCK_GAP_PX + fieldCount * CAPTION_LINE_HEIGHT_PX;
}
