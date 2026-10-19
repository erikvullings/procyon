import type { Location } from './location';

/** One metadata tag of an image (task 0273). */
export interface ImageTag {
  readonly name: string;
  /** Human-readable value. */
  readonly value: string;
  /** The raw stored value when it differs from `value`. */
  readonly raw?: string | null;
}

/** Tags of one namespace: EXIF, GPS, IPTC, XMP, ICC, PNG or File. */
export interface ImageTagGroup {
  readonly namespace: string;
  readonly tags: readonly ImageTag[];
}

export interface GpsPosition {
  readonly latitude: number;
  readonly longitude: number;
}

/** All metadata of an image as parsed by the backend. */
export interface ImageMetadata {
  readonly format?: string | null;
  /** False when the format cannot be parsed. */
  readonly supported: boolean;
  readonly groups: readonly ImageTagGroup[];
  readonly gps?: GpsPosition | null;
  /** Field ids this file's format can store and write; empty when it cannot be edited. */
  readonly editableFields?: readonly string[];
}

/** GPS result of one image from the bulk scan (task 0276). */
export interface ImageGpsEntry {
  readonly location: Location;
  readonly gps?: GpsPosition | null;
  /** Capture time `YYYY-MM-DDTHH:MM:SS` without a time zone. */
  readonly takenAt?: string | null;
  readonly unreadable?: boolean;
}

export interface CaptureTime {
  year: number;
  month: number;
  day: number;
  hour: number;
  minute: number;
  second: number;
}

/**
 * Changes applied to every file of an `editImageMetadata` operation (task 0274). A field left out
 * stays unchanged; ids in `clearFields` are removed.
 */
export interface ImageMetadataEdit {
  removeAll?: boolean;
  removeLocation?: boolean;
  clearFields?: string[];
  /** Keep the original as `<name>.orig`, which also makes the edit undoable. */
  backupOriginal?: boolean;
  dateTaken?: CaptureTime;
  shiftDateSeconds?: number;
  gps?: GpsPosition;
  title?: string;
  description?: string;
  copyright?: string;
  author?: string;
  keywords?: string[];
  rating?: number;
  orientation?: number;
}
