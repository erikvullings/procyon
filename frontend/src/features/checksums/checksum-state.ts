import type {
  ChecksumAlgorithm,
  ChecksumEntry,
  DuplicateGroup,
  Location,
  VerificationReport,
} from '../../models';

/** Live state for the checksum results panel (spec §18, task 0077). */
export interface ChecksumState {
  readonly jobId?: string;
  readonly algorithms: readonly ChecksumAlgorithm[];
  readonly entries: readonly ChecksumEntry[];
  readonly totalEntries: number;
  readonly isComplete: boolean;
  readonly isCancelled: boolean;
  /** Report from the most recent "verify against checksum file" run. */
  readonly verification?: VerificationReport;
  /** Where the results were most recently written, for a "saved to …" hint. */
  readonly savedTo?: Location;
  readonly error?: string;
}

/** Live state for the duplicate-review panel. */
export interface DuplicateState {
  readonly scanId?: string;
  readonly roots: readonly Location[];
  readonly groups: readonly DuplicateGroup[];
  readonly isComplete: boolean;
  readonly isCancelled: boolean;
  readonly warningsCount: number;
  /** URIs the user has ticked for deletion. */
  readonly selectedUris: ReadonlySet<string>;
  readonly error?: string;
}

export function initialChecksumState(): ChecksumState {
  return {
    algorithms: [],
    entries: [],
    totalEntries: 0,
    isComplete: false,
    isCancelled: false,
  };
}

export function initialDuplicateState(): DuplicateState {
  return {
    roots: [],
    groups: [],
    isComplete: false,
    isCancelled: false,
    warningsCount: 0,
    selectedUris: new Set(),
  };
}

/** Replaces any previous job with a freshly started one. */
export function withChecksumJobStarted(
  jobId: string,
  algorithms: readonly ChecksumAlgorithm[],
  totalEntries: number,
): ChecksumState {
  return {
    jobId,
    algorithms,
    entries: [],
    totalEntries,
    isComplete: false,
    isCancelled: false,
  };
}

/**
 * Merges a streamed results batch into the running job. A no-op if `jobId`
 * does not match the tracked job (a stale batch arriving late).
 */
export function withChecksumBatch(
  state: ChecksumState,
  jobId: string,
  entries: readonly ChecksumEntry[],
  isComplete: boolean,
  isCancelled: boolean,
): ChecksumState {
  if (state.jobId !== jobId) return state;
  // Entries are appended rather than replaced: each batch carries only what
  // was hashed since the previous one.
  return { ...state, entries: [...state.entries, ...entries], isComplete, isCancelled };
}

export function withChecksumCleared(): ChecksumState {
  return initialChecksumState();
}

export function withChecksumError(state: ChecksumState, message: string): ChecksumState {
  return { ...state, error: message };
}

/** Records a successful save and clears any stale error. */
export function withChecksumSaved(state: ChecksumState, location: Location): ChecksumState {
  const { error: _cleared, ...rest } = state;
  return { ...rest, savedTo: location };
}

export function withVerificationReport(
  state: ChecksumState,
  report: VerificationReport,
): ChecksumState {
  if (state.jobId !== report.jobId) return state;
  return { ...state, verification: report };
}

export function withDuplicateScanStarted(
  scanId: string,
  roots: readonly Location[],
): DuplicateState {
  return {
    scanId,
    roots,
    groups: [],
    isComplete: false,
    isCancelled: false,
    warningsCount: 0,
    selectedUris: new Set(),
  };
}

/** Applies the terminal `duplicates.resultsReady` event to the tracked scan. */
export function withDuplicateResults(
  state: DuplicateState,
  scanId: string,
  groups: readonly DuplicateGroup[],
  isCancelled: boolean,
  warningsCount: number,
): DuplicateState {
  if (state.scanId !== scanId) return state;
  return { ...state, groups, isComplete: true, isCancelled, warningsCount };
}

export function withDuplicateCleared(): DuplicateState {
  return initialDuplicateState();
}

export function withDuplicateError(state: DuplicateState, message: string): DuplicateState {
  return { ...state, error: message };
}

/** Toggles one path's "delete this copy" tick. */
export function withDuplicateSelectionToggled(state: DuplicateState, uri: string): DuplicateState {
  const next = new Set(state.selectedUris);
  if (next.has(uri)) next.delete(uri);
  else next.add(uri);
  return { ...state, selectedUris: next };
}

export function withDuplicateSelectionCleared(state: DuplicateState): DuplicateState {
  return { ...state, selectedUris: new Set() };
}

/** Total bytes a completed scan says could be reclaimed. */
export function totalReclaimableBytes(state: DuplicateState): number {
  return state.groups.reduce((total, group) => total + group.reclaimableBytes, 0);
}

/**
 * Every location the user has ticked, as a flat list ready for the normal
 * delete flow.
 */
export function selectedLocations(state: DuplicateState): readonly Location[] {
  const byUri = new Map<string, Location>();
  for (const group of state.groups) {
    for (const location of group.distinctLocations) byUri.set(location.uri, location);
    for (const cluster of group.hardlinkClusters) {
      for (const location of cluster.locations) byUri.set(location.uri, location);
    }
  }
  return [...state.selectedUris]
    .map((uri) => byUri.get(uri))
    .filter((location): location is Location => location !== undefined);
}

/**
 * Whether ticking `uri` would leave its group with no copy at all.
 *
 * Duplicate review must never let the user delete every copy of a piece of
 * content by accident, so the UI keeps at least one path per group untickable
 * once the rest are ticked. Hardlink clusters count as a single copy, since
 * deleting one of their paths reclaims nothing and leaves the content intact.
 */
export function wouldDeleteEveryCopy(state: DuplicateState, uri: string): boolean {
  const group = state.groups.find(
    (candidate) =>
      candidate.distinctLocations.some((location) => location.uri === uri) ||
      candidate.hardlinkClusters.some((cluster) =>
        cluster.locations.some((location) => location.uri === uri),
      ),
  );
  if (group === undefined) return false;
  if (state.selectedUris.has(uri)) return false;

  const distinctSurviving = group.distinctLocations.filter(
    (location) => location.uri !== uri && !state.selectedUris.has(location.uri),
  ).length;
  const clustersSurviving = group.hardlinkClusters.filter((cluster) =>
    cluster.locations.some(
      (location) => location.uri !== uri && !state.selectedUris.has(location.uri),
    ),
  ).length;
  return distinctSurviving + clustersSurviving === 0;
}
