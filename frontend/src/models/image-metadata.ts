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
}
