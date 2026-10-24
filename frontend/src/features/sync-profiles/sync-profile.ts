import type {
  ConflictPolicy,
  Location,
  SyncPlanItem,
  SyncProfile,
  SyncRunOutcome,
  SyncRunSummary,
  SyncRunTrigger,
  SyncSchedule,
} from '../../models';

/** Shortest and longest schedule interval the backend accepts (minutes). */
export const MIN_INTERVAL_MINUTES = 5;
export const MAX_INTERVAL_MINUTES = 10_080;
export const MAX_PROFILE_NAME_CHARS = 80;
/** A prepared plan older than this must be refreshed before it can be applied. */
export const PLAN_MAX_AGE_MS = 15 * 60 * 1000;

export type ProfileValidationIssue =
  | 'blankName'
  | 'duplicateName'
  | 'missingLocation'
  | 'sameRoots'
  | 'intervalOutOfRange';

/** A new profile with the safest defaults: two-way (no deletions) and ask on conflicts. */
export function newSyncProfile(
  id: string,
  name: string,
  left: Location,
  right: Location,
): SyncProfile {
  return {
    id,
    name,
    left,
    right,
    mode: 'twoWayUpdate',
    criteria: 'sizeAndTimestamp',
    showHidden: false,
    include: [],
    exclude: [],
    conflictPolicy: 'ask',
    schedule: null,
    lastRun: null,
  };
}

/** Copies a profile under a unique "name copy" label; the copy is unscheduled with no history. */
export function duplicateSyncProfile(
  profiles: readonly SyncProfile[],
  sourceId: string,
  newId: string,
): SyncProfile | undefined {
  const source = profiles.find((profile) => profile.id === sourceId);
  if (source === undefined) return undefined;
  const taken = new Set(profiles.map((profile) => profile.name.toLocaleLowerCase()));
  let name = `${source.name} copy`;
  for (let index = 2; taken.has(name.toLocaleLowerCase()); index += 1) {
    name = `${source.name} copy ${index}`;
  }
  return {
    ...source,
    id: newId,
    name: name.slice(0, MAX_PROFILE_NAME_CHARS),
    include: [...source.include],
    exclude: [...source.exclude],
    schedule: null,
    lastRun: null,
  };
}

export function validateSyncProfile(
  profile: SyncProfile,
  others: readonly SyncProfile[],
): ProfileValidationIssue[] {
  const issues: ProfileValidationIssue[] = [];
  const name = profile.name.trim();
  if (name === '') issues.push('blankName');
  else if (
    others.some(
      (other) =>
        other.id !== profile.id &&
        other.name.trim().toLocaleLowerCase() === name.toLocaleLowerCase(),
    )
  ) {
    issues.push('duplicateName');
  }
  if (profile.left.uri.trim() === '' || profile.right.uri.trim() === '') {
    issues.push('missingLocation');
  } else if (
    profile.left.providerId === profile.right.providerId &&
    profile.left.uri === profile.right.uri
  ) {
    issues.push('sameRoots');
  }
  if (
    profile.schedule !== null &&
    (!Number.isInteger(profile.schedule.intervalMinutes) ||
      profile.schedule.intervalMinutes < MIN_INTERVAL_MINUTES ||
      profile.schedule.intervalMinutes > MAX_INTERVAL_MINUTES)
  ) {
    issues.push('intervalOutOfRange');
  }
  return issues;
}

/** Inserts or replaces (by id) a profile. */
export function upsertSyncProfile(
  profiles: readonly SyncProfile[],
  profile: SyncProfile,
): readonly SyncProfile[] {
  return profiles.some((current) => current.id === profile.id)
    ? profiles.map((current) => (current.id === profile.id ? profile : current))
    : [...profiles, profile];
}

export function removeSyncProfile(
  profiles: readonly SyncProfile[],
  id: string,
): readonly SyncProfile[] {
  return profiles.filter((profile) => profile.id !== id);
}

/** Records a run summary on the matching profile. */
export function withLastRun(
  profiles: readonly SyncProfile[],
  id: string,
  lastRun: SyncRunSummary,
): readonly SyncProfile[] {
  return profiles.map((profile) => (profile.id === id ? { ...profile, lastRun } : profile));
}

export function runSummary(params: {
  readonly now: Date;
  readonly trigger: SyncRunTrigger;
  readonly outcome: SyncRunOutcome;
  readonly plannedItems: number;
  readonly appliedItems?: number;
  readonly message?: string;
}): SyncRunSummary {
  return {
    at: params.now.toISOString(),
    trigger: params.trigger,
    outcome: params.outcome,
    plannedItems: params.plannedItems,
    appliedItems: params.appliedItems ?? 0,
    message: params.message ?? null,
  };
}

/**
 * Whether an enabled schedule is due. The last run counts whatever its outcome (a failed run
 * waits a full interval too), so an unavailable location never causes a retry storm.
 */
export function isScheduleDue(profile: SyncProfile, now: Date): boolean {
  const schedule: SyncSchedule | null = profile.schedule;
  if (schedule === null || !schedule.enabled) return false;
  if (profile.lastRun === null) return true;
  const last = Date.parse(profile.lastRun.at);
  if (Number.isNaN(last)) return true;
  return now.getTime() - last >= schedule.intervalMinutes * 60_000;
}

/** Profiles whose schedule is due, in the given order. */
export function dueProfiles(profiles: readonly SyncProfile[], now: Date): SyncProfile[] {
  return profiles.filter((profile) => isScheduleDue(profile, now));
}

/**
 * Rows that may run without a human: copies onto a side that does not have the entry yet. Anything
 * that overwrites a differing destination or deletes stays in the review list, whatever the
 * profile's conflict policy says.
 */
export function isAdditionOnly(item: SyncPlanItem): boolean {
  return (
    (item.action === 'copyLeftToRight' && item.right === undefined) ||
    (item.action === 'copyRightToLeft' && item.left === undefined)
  );
}

export interface ScheduledSplit {
  /** Applied without confirmation. */
  readonly automatic: readonly SyncPlanItem[];
  /** Left for a person to review. */
  readonly review: readonly SyncPlanItem[];
}

/** Applies the schedule's confirmation policy to a freshly prepared plan. */
export function splitScheduledPlan(
  schedule: SyncSchedule | null,
  items: readonly SyncPlanItem[],
): ScheduledSplit {
  const actionable = items.filter((item) => item.action !== 'skip');
  if (schedule?.confirmation !== 'autoApplyAdditions') {
    return { automatic: [], review: actionable };
  }
  return {
    automatic: actionable.filter(isAdditionOnly),
    review: actionable.filter((item) => !isAdditionOnly(item)),
  };
}

export function isPlanStale(generatedAtMs: number, nowMs: number): boolean {
  return nowMs - generatedAtMs > PLAN_MAX_AGE_MS;
}

/** The operation-engine policy for a profile's stored conflict default. */
export function operationConflictPolicy(policy: SyncProfile['conflictPolicy']): ConflictPolicy {
  return policy === 'keepBoth' ? 'renameNew' : policy;
}

/** Splits a multi-line pattern list into trimmed, non-blank, de-duplicated patterns. */
export function parsePatternList(text: string): string[] {
  return [
    ...new Set(
      text
        .split(/[\n;]/u)
        .map((line) => line.trim())
        .filter((line) => line !== ''),
    ),
  ];
}
