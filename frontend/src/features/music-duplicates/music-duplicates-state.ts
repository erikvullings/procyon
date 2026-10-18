import type { MusicDuplicateGroup } from '../../models';

/** Complete state of the music duplicate review panel (task 0265). */
export interface MusicDuplicatesState {
  readonly scanId: string | undefined;
  readonly groups: readonly MusicDuplicateGroup[];
  readonly phase: 'listing' | 'hashing' | 'tagging' | 'grouping' | 'done';
  readonly filesFound: number;
  readonly filesProcessed: number;
  readonly unreadableTags: number;
  readonly isComplete: boolean;
  readonly isCancelled: boolean;
  readonly selectedUris: ReadonlySet<string>;
  readonly removing: boolean;
  readonly error: string | undefined;
}

export const INITIAL_MUSIC_DUPLICATES_STATE: MusicDuplicatesState = {
  scanId: undefined,
  groups: [],
  phase: 'listing',
  filesFound: 0,
  filesProcessed: 0,
  unreadableTags: 0,
  isComplete: false,
  isCancelled: false,
  selectedUris: new Set(),
  removing: false,
  error: undefined,
};

/** URIs of every file except each group's suggested keeper. */
export function suggestedRemovals(groups: readonly MusicDuplicateGroup[]): Set<string> {
  const removals = new Set<string>();
  for (const group of groups) {
    group.files.forEach((file, index) => {
      if (index !== group.keepIndex) removals.add(file.location.uri);
    });
  }
  return removals;
}

/** Whether removing `uri` as well would leave its group without any copy. */
export function wouldRemoveEveryCopy(
  groups: readonly MusicDuplicateGroup[],
  selected: ReadonlySet<string>,
  uri: string,
): boolean {
  const group = groups.find((candidate) =>
    candidate.files.some((file) => file.location.uri === uri),
  );
  if (group === undefined) return false;
  return group.files.every((file) => file.location.uri === uri || selected.has(file.location.uri));
}

/** Drops removed files; a group with fewer than two files left is no longer a duplicate. */
export function withoutRemoved(
  groups: readonly MusicDuplicateGroup[],
  removed: ReadonlySet<string>,
): MusicDuplicateGroup[] {
  return groups
    .map((group) => {
      const files = group.files.filter((file) => !removed.has(file.location.uri));
      const keeper = group.files[group.keepIndex];
      const keepIndex = Math.max(
        0,
        files.findIndex((file) => file.location.uri === keeper?.location.uri),
      );
      return { ...group, files, keepIndex, reclaimableBytes: 0 };
    })
    .filter((group) => group.files.length >= 2)
    .map((group) => ({
      ...group,
      reclaimableBytes: group.files.reduce(
        (total, file, index) => (index === group.keepIndex ? total : total + file.size),
        0,
      ),
    }));
}

export function totalReclaimableBytes(groups: readonly MusicDuplicateGroup[]): number {
  return groups.reduce((total, group) => total + group.reclaimableBytes, 0);
}
