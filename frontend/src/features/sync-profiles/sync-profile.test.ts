import { describe, expect, it } from 'vitest';
import type { Location, SyncPlanItem, SyncProfile } from '../../models';
import {
  dueProfiles,
  duplicateSyncProfile,
  isAdditionOnly,
  isPlanStale,
  isScheduleDue,
  newSyncProfile,
  operationConflictPolicy,
  PLAN_MAX_AGE_MS,
  parsePatternList,
  removeSyncProfile,
  runSummary,
  splitScheduledPlan,
  upsertSyncProfile,
  validateSyncProfile,
  withLastRun,
} from './sync-profile';

const left: Location = { providerId: 'local', uri: 'file:///a' };
const right: Location = { providerId: 'sftp', uri: 'sftp://me@nas/b' };
const profile = (id: string, name: string, extra: Partial<SyncProfile> = {}): SyncProfile => ({
  ...newSyncProfile(id, name, left, right),
  ...extra,
});
const side = { kind: 'file' as const, size: 1 };
const item = (
  action: SyncPlanItem['action'],
  hasLeft: boolean,
  hasRight: boolean,
): SyncPlanItem => ({
  relativePath: `${action}-${hasLeft}-${hasRight}`,
  status: 'differentSize',
  action,
  ...(hasLeft ? { left: side } : {}),
  ...(hasRight ? { right: side } : {}),
});

describe('profile editing', () => {
  it('new profiles default to the safest settings', () => {
    const created = newSyncProfile('1', 'Photos', left, right);
    expect(created.mode).toBe('twoWayUpdate');
    expect(created.conflictPolicy).toBe('ask');
    expect(created.schedule).toBeNull();
  });

  it('duplicates under a unique name without schedule or history', () => {
    const source = profile('1', 'Photos', {
      include: ['*.jpg'],
      schedule: { enabled: true, intervalMinutes: 30, confirmation: 'autoApplyAdditions' },
      lastRun: runSummary({
        now: new Date(),
        trigger: 'manual',
        outcome: 'applied',
        plannedItems: 1,
      }),
    });
    const existing = [source, profile('2', 'Photos copy')];
    const copy = duplicateSyncProfile(existing, '1', '3');
    expect(copy?.name).toBe('Photos copy 2');
    expect(copy?.id).toBe('3');
    expect(copy?.schedule).toBeNull();
    expect(copy?.lastRun).toBeNull();
    expect(copy?.include).toEqual(['*.jpg']);
    expect(duplicateSyncProfile(existing, 'missing', '4')).toBeUndefined();
  });

  it('upserts, removes and records last runs by id', () => {
    const a = profile('1', 'A');
    const b = profile('2', 'B');
    const list = upsertSyncProfile([a, b], { ...a, name: 'A2' });
    expect(list.map((p) => p.name)).toEqual(['A2', 'B']);
    expect(upsertSyncProfile(list, profile('3', 'C'))).toHaveLength(3);
    expect(removeSyncProfile(list, '1').map((p) => p.id)).toEqual(['2']);
    const run = runSummary({
      now: new Date(0),
      trigger: 'manual',
      outcome: 'upToDate',
      plannedItems: 0,
    });
    expect(withLastRun(list, '2', run)[1]?.lastRun).toEqual(run);
    expect(withLastRun(list, '2', run)[0]?.lastRun).toBeNull();
  });

  it('validates names, roots and the schedule interval', () => {
    const others = [profile('2', 'photos')];
    expect(validateSyncProfile(profile('1', ' '), others)).toContain('blankName');
    expect(validateSyncProfile(profile('1', 'Photos'), others)).toContain('duplicateName');
    expect(validateSyncProfile(profile('2', 'Photos'), others)).toEqual([]);
    expect(validateSyncProfile(profile('1', 'X', { right: { ...left } }), [])).toContain(
      'sameRoots',
    );
    expect(
      validateSyncProfile(profile('1', 'X', { left: { providerId: 'local', uri: '' } }), []),
    ).toContain('missingLocation');
    const schedule = { enabled: true, intervalMinutes: 1, confirmation: 'required' as const };
    expect(validateSyncProfile(profile('1', 'X', { schedule }), [])).toContain(
      'intervalOutOfRange',
    );
  });

  it('parses pattern lists', () => {
    expect(parsePatternList(' *.tmp\n\nnode_modules; *.tmp ')).toEqual(['*.tmp', 'node_modules']);
  });

  it('maps keepBoth to the operation engine rename policy', () => {
    expect(operationConflictPolicy('keepBoth')).toBe('renameNew');
    expect(operationConflictPolicy('skip')).toBe('skip');
  });
});

describe('scheduling', () => {
  const schedule = { enabled: true, intervalMinutes: 60, confirmation: 'required' as const };
  const now = new Date('2026-10-24T12:00:00Z');
  const ran = (at: string) =>
    runSummary({ now: new Date(at), trigger: 'scheduled', outcome: 'failed', plannedItems: 0 });

  it('never fires without an explicit enabled schedule', () => {
    expect(isScheduleDue(profile('1', 'A'), now)).toBe(false);
    expect(
      isScheduleDue(profile('1', 'A', { schedule: { ...schedule, enabled: false } }), now),
    ).toBe(false);
  });

  it('fires when never run or when the interval elapsed since any run, including a failed one', () => {
    expect(isScheduleDue(profile('1', 'A', { schedule }), now)).toBe(true);
    const recent = profile('1', 'A', { schedule, lastRun: ran('2026-10-24T11:30:00Z') });
    expect(isScheduleDue(recent, now)).toBe(false);
    const old = profile('1', 'A', { schedule, lastRun: ran('2026-10-24T10:59:59Z') });
    expect(isScheduleDue(old, now)).toBe(true);
  });

  it('selects only due profiles from an unordered mix', () => {
    const due = profile('b', 'B', { schedule, lastRun: ran('2026-10-23T00:00:00Z') });
    const notDue = profile('c', 'C', { schedule, lastRun: ran('2026-10-24T11:59:00Z') });
    const unscheduled = profile('a', 'A');
    expect(dueProfiles([notDue, unscheduled, due], now).map((p) => p.id)).toEqual(['b']);
  });
});

describe('confirmation policy', () => {
  const rows = [
    item('copyLeftToRight', true, false),
    item('copyLeftToRight', true, true),
    item('deleteRight', false, true),
    item('copyRightToLeft', false, true),
    item('copyRightToLeft', true, true),
    item('skip', true, true),
  ];

  it('treats only copies onto a side lacking the entry as additions', () => {
    expect(rows.map(isAdditionOnly)).toEqual([true, false, false, true, false, false]);
  });

  it('requires a human for everything by default', () => {
    const split = splitScheduledPlan(
      { enabled: true, intervalMinutes: 60, confirmation: 'required' },
      rows,
    );
    expect(split.automatic).toEqual([]);
    expect(split.review).toHaveLength(5);
    expect(splitScheduledPlan(null, rows).automatic).toEqual([]);
  });

  it('auto-applies additions only, never overwrites or deletions', () => {
    const split = splitScheduledPlan(
      { enabled: true, intervalMinutes: 60, confirmation: 'autoApplyAdditions' },
      rows,
    );
    expect(split.automatic.map((r) => r.action)).toEqual(['copyLeftToRight', 'copyRightToLeft']);
    expect(split.review.map((r) => r.action)).toEqual([
      'copyLeftToRight',
      'deleteRight',
      'copyRightToLeft',
    ]);
  });
});

describe('staleness', () => {
  it('flags plans older than the maximum age', () => {
    expect(isPlanStale(1_000, 1_000 + PLAN_MAX_AGE_MS)).toBe(false);
    expect(isPlanStale(1_000, 1_001 + PLAN_MAX_AGE_MS)).toBe(true);
  });
});
