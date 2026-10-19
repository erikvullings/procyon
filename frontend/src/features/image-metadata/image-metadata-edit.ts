import type { CaptureTime, EntrySummary, ImageMetadataEdit } from '../../models';

/** Opens "Edit image metadata…" for the selected images (task 0274). */
export const EDIT_IMAGE_METADATA_ACTION_ID = 'client.editImageMetadata';

/** Extensions the backend can rewrite losslessly; it stays the authority per file. */
const EDITABLE_EXTENSIONS: ReadonlySet<string> = new Set(['jpg', 'jpeg', 'tif', 'tiff', 'webp']);
/** Extensions that carry XMP/IPTC, so title, keywords and rating can be stored. */
const RICH_EXTENSIONS: ReadonlySet<string> = new Set(['jpg', 'jpeg']);

function extensionOf(entry: EntrySummary): string {
  return (entry.extension ?? entry.name.split('.').pop() ?? '').toLowerCase();
}

export function isMetadataEditable(entry: EntrySummary): boolean {
  return entry.kind === 'file' && EDITABLE_EXTENSIONS.has(extensionOf(entry));
}

/** Whether title, keywords and rating can be written to every one of these entries. */
export function supportsRichFields(entries: readonly EntrySummary[]): boolean {
  return entries.length > 0 && entries.every((entry) => RICH_EXTENSIONS.has(extensionOf(entry)));
}

/** What the user typed in the dialog; blank text means "leave unchanged". */
export interface ImageMetadataForm {
  removeAll: boolean;
  removeLocation: boolean;
  backupOriginal: boolean;
  /** `YYYY-MM-DDTHH:MM[:SS]` from a datetime-local input. */
  dateTaken: string;
  /** Camera clock correction, in minutes, applied to each file's own capture time. */
  shiftMinutes: string;
  latitude: string;
  longitude: string;
  title: string;
  description: string;
  copyright: string;
  author: string;
  /** Comma separated. */
  keywords: string;
  /** `''` unchanged, `0`-`5`. */
  rating: string;
  /** `''` unchanged, `1`-`8`. */
  orientation: string;
  /** Field ids to remove. */
  cleared: ReadonlySet<string>;
}

export function emptyForm(): ImageMetadataForm {
  return {
    removeAll: false,
    removeLocation: false,
    backupOriginal: true,
    dateTaken: '',
    shiftMinutes: '',
    latitude: '',
    longitude: '',
    title: '',
    description: '',
    copyright: '',
    author: '',
    keywords: '',
    rating: '',
    orientation: '',
    cleared: new Set(),
  };
}

export function parseCaptureTime(text: string): CaptureTime | undefined {
  const match = /^(\d{4})-(\d{2})-(\d{2})[T ](\d{2}):(\d{2})(?::(\d{2}))?$/.exec(text.trim());
  if (match === null) return undefined;
  const [year, month, day, hour, minute, second] = match.slice(1).map((part) => Number(part ?? 0));
  const time = {
    year: year ?? 0,
    month: month ?? 0,
    day: day ?? 0,
    hour: hour ?? 0,
    minute: minute ?? 0,
    second: second ?? 0,
  };
  const valid =
    time.year > 0 &&
    time.month >= 1 &&
    time.month <= 12 &&
    time.day >= 1 &&
    time.day <= 31 &&
    time.hour < 24 &&
    time.minute < 60 &&
    time.second < 60;
  return valid ? time : undefined;
}

function parseCoordinate(text: string, limit: number): number | undefined {
  const value = Number(text.trim().replace(',', '.'));
  return text.trim() !== '' && Number.isFinite(value) && Math.abs(value) <= limit
    ? value
    : undefined;
}

/** Problems that keep the form from being submitted, as i18n keys of the `imageEdit` group. */
export type FormProblem = 'invalidDate' | 'invalidShift' | 'invalidGps';

export function formProblems(form: ImageMetadataForm): readonly FormProblem[] {
  const problems: FormProblem[] = [];
  if (form.dateTaken.trim() !== '' && parseCaptureTime(form.dateTaken) === undefined) {
    problems.push('invalidDate');
  }
  if (form.shiftMinutes.trim() !== '' && !Number.isInteger(Number(form.shiftMinutes))) {
    problems.push('invalidShift');
  }
  const hasLatitude = form.latitude.trim() !== '';
  const hasLongitude = form.longitude.trim() !== '';
  if (
    hasLatitude !== hasLongitude ||
    (hasLatitude &&
      (parseCoordinate(form.latitude, 90) === undefined ||
        parseCoordinate(form.longitude, 180) === undefined))
  ) {
    problems.push('invalidGps');
  }
  return problems;
}

/** Builds the backend edit from the form; `undefined` when it would change nothing. */
export function buildImageMetadataEdit(form: ImageMetadataForm): ImageMetadataEdit | undefined {
  const edit: ImageMetadataEdit = { backupOriginal: form.backupOriginal };
  let changes = false;
  const mark = (): void => {
    changes = true;
  };
  if (form.removeAll) {
    edit.removeAll = true;
    mark();
  }
  if (form.removeLocation) {
    edit.removeLocation = true;
    mark();
  }
  const cleared = [...form.cleared];
  if (cleared.length > 0) {
    edit.clearFields = cleared;
    mark();
  }
  const date = parseCaptureTime(form.dateTaken);
  if (date !== undefined) {
    edit.dateTaken = date;
    mark();
  }
  const shift = Number(form.shiftMinutes);
  if (form.shiftMinutes.trim() !== '' && Number.isInteger(shift) && shift !== 0) {
    edit.shiftDateSeconds = shift * 60;
    mark();
  }
  const latitude = parseCoordinate(form.latitude, 90);
  const longitude = parseCoordinate(form.longitude, 180);
  if (latitude !== undefined && longitude !== undefined) {
    edit.gps = { latitude, longitude };
    mark();
  }
  for (const key of ['title', 'description', 'copyright', 'author'] as const) {
    const value = form[key].trim();
    if (value !== '') {
      edit[key] = value;
      mark();
    }
  }
  const keywords = form.keywords
    .split(',')
    .map((keyword) => keyword.trim())
    .filter((keyword) => keyword !== '');
  if (keywords.length > 0) {
    edit.keywords = keywords;
    mark();
  }
  if (form.rating !== '') {
    edit.rating = Number(form.rating);
    mark();
  }
  if (form.orientation !== '') {
    edit.orientation = Number(form.orientation);
    mark();
  }
  return changes ? edit : undefined;
}
