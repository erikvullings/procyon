import type {
  Location,
  SnapshotDiffEntry,
  SnapshotJobPage,
  StartSnapshotJobRequest,
} from '../../models';

/** State of the folder snapshot panel (task 0268). */
export interface SnapshotPanelState {
  /** `'starting'` between pressing the button and the backend returning an id. */
  readonly jobId: string | undefined;
  readonly request: StartSnapshotJobRequest | undefined;
  /** Latest progress; its `entries` are replaced by {@link entries} once complete. */
  readonly page: SnapshotJobPage | undefined;
  readonly entries: readonly SnapshotDiffEntry[];
  readonly error: string | undefined;
}

export const INITIAL_SNAPSHOT_PANEL_STATE: SnapshotPanelState = {
  jobId: undefined,
  request: undefined,
  page: undefined,
  entries: [],
  error: undefined,
};

export const SNAPSHOT_EXTENSION = 'procyon-snapshot';

/** True for a file name ending in `.procyon-snapshot`. */
export function isSnapshotFileName(name: string): boolean {
  return name.toLowerCase().endsWith(`.${SNAPSHOT_EXTENSION}`);
}

/** Default snapshot file name for a folder: `<folder>.procyon-snapshot`. */
export function defaultSnapshotName(folderName: string): string {
  const base = folderName.trim().length > 0 ? folderName.trim() : 'snapshot';
  return `${base}.${SNAPSHOT_EXTENSION}`;
}

/** Appends the snapshot extension unless it is already present. */
export function snapshotFileName(name: string): string {
  const trimmed = name.trim();
  return isSnapshotFileName(trimmed) ? trimmed : `${trimmed}.${SNAPSHOT_EXTENSION}`;
}

/** Joins a directory location and a file name into a child location. */
export function childLocation(directory: Location, name: string): Location {
  return {
    ...directory,
    uri: `${directory.uri.replace(/\/+$/u, '')}/${encodeURIComponent(name)}`,
  };
}
