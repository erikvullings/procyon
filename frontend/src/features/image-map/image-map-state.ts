import type { EntrySummary } from '../../models';
import type { GeoImage } from './gps-scan';

export interface PendingPlacement {
  readonly entry: EntrySummary;
  readonly latitude: number;
  readonly longitude: number;
}

/** Everything the "Show on map" panel displays (task 0276). */
export interface ImageMapState {
  readonly open: boolean;
  readonly scanning: boolean;
  readonly done: number;
  readonly total: number;
  readonly located: readonly GeoImage[];
  /** Images without GPS tags, listed separately. */
  readonly unlocated: readonly EntrySummary[];
  readonly unreadable: readonly EntrySummary[];
  /** True when the image limit cut the folder short. */
  readonly truncated: boolean;
  /** Whether subfolders are included (only offered when the source is a folder). */
  readonly recursive: boolean;
  readonly foldersOffered: boolean;
  /** An unlocated image waiting for the user to click its position on the map. */
  readonly placing: EntrySummary | undefined;
  /** A move or placement waiting for confirmation. */
  readonly pending: PendingPlacement | undefined;
  readonly failure: string | undefined;
}

export const INITIAL_IMAGE_MAP_STATE: ImageMapState = {
  open: false,
  scanning: false,
  done: 0,
  total: 0,
  located: [],
  unlocated: [],
  unreadable: [],
  truncated: false,
  recursive: false,
  foldersOffered: false,
  placing: undefined,
  pending: undefined,
  failure: undefined,
};
