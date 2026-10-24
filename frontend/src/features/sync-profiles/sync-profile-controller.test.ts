import { describe, expect, it } from 'vitest';
import { MockFileManagerClient } from '../../api/client/mock-file-manager-client';
import type { Location, Settings, SyncProfile } from '../../models';
import { newSyncProfile } from './sync-profile';
import {
  createSyncProfileController,
  describeRunError,
  type SyncProfileController,
} from './sync-profile-controller';
import { SyncRunError } from './sync-profile-runner';

const docs: Location = { providerId: 'mock', uri: 'mock:///Documents' };
const projects: Location = { providerId: 'mock', uri: 'mock:///Documents/Projects' };
const base = (id: string, name: string, extra: Partial<SyncProfile> = {}): SyncProfile => ({
  ...newSyncProfile(id, name, docs, projects),
  ...extra,
});

async function setup(
  initial: readonly SyncProfile[],
  at = new Date('2026-10-24T12:00:00Z'),
  client: MockFileManagerClient = new MockFileManagerClient(),
) {
  let settings: Settings = { ...(await client.getSettings()), syncProfiles: initial };
  const clock = { now: at };
  const notices: string[] = [];
  let ids = 0;
  const applyCalls: { count: number; actions: string[] }[] = [];
  const originalApply = client.applySyncPlan.bind(client);
  client.applySyncPlan = (comparisonId, request, signal) => {
    applyCalls.push({ count: request.items.length, actions: request.items.map((i) => i.action) });
    return originalApply(comparisonId, request, signal);
  };
  const controller: SyncProfileController = createSyncProfileController({
    getClient: () => client,
    getSettings: () => settings,
    updateSettings: (update) => {
      settings = update(settings);
      return Promise.resolve();
    },
    getRunTarget: () => ({ workspaceId: 'workspace-1', paneId: 'pane-left' }),
    notify: (message) => notices.push(message),
    redraw: () => undefined,
    newId: () => `id-${++ids}`,
    now: () => clock.now,
    sleep: () => Promise.resolve(),
  });
  return { controller, settings: () => settings, notices, applyCalls, clock };
}

describe('profile management', () => {
  it('creates, edits, duplicates and deletes profiles through settings', async () => {
    const { controller, settings } = await setup([]);
    expect(await controller.save(base('1', 'Docs'))).toEqual([]);
    expect(await controller.save(base('2', 'docs'))).toEqual(['duplicateName']);
    expect(await controller.save(base('1', 'Docs renamed'))).toEqual([]);
    expect(settings().syncProfiles.map((p) => p.name)).toEqual(['Docs renamed']);
    await controller.duplicate('1');
    expect(settings().syncProfiles.map((p) => p.name)).toEqual([
      'Docs renamed',
      'Docs renamed copy',
    ]);
    await controller.remove('1');
    expect(settings().syncProfiles.map((p) => p.id)).toEqual(['id-1']);
  });
});

describe('manual runs', () => {
  it('always prepares a dry-run plan first and mutates nothing until confirmed', async () => {
    const { controller, settings, applyCalls } = await setup([base('1', 'Docs')]);
    await controller.run('1');
    const review = controller.pendingReview();
    expect(review?.open).toBe(true);
    expect(review?.plan.items.length).toBeGreaterThan(0);
    expect(applyCalls).toHaveLength(0);
    expect(settings().syncProfiles[0]?.lastRun?.outcome).toBe('planned');

    await controller.confirmReview(review?.plan.items ?? []);
    expect(applyCalls).toHaveLength(1);
    expect(controller.pendingReview()).toBeUndefined();
    expect(settings().syncProfiles[0]?.lastRun).toMatchObject({
      outcome: 'applied',
      trigger: 'manual',
    });
  });

  it('records a dismissed plan without applying', async () => {
    const { controller, settings, applyCalls } = await setup([base('1', 'Docs')]);
    await controller.run('1');
    await controller.dismissReview();
    expect(applyCalls).toHaveLength(0);
    expect(settings().syncProfiles[0]?.lastRun?.outcome).toBe('dismissed');
  });

  it('records an actionable failure for a missing remote connection and changes nothing', async () => {
    const remote = { providerId: 'sftp', uri: 'sftp://me@nas/x' };
    class OfflineRemote extends MockFileManagerClient {
      override listDirectory(
        ...args: Parameters<MockFileManagerClient['listDirectory']>
      ): ReturnType<MockFileManagerClient['listDirectory']> {
        return args[0].location.providerId === 'sftp'
          ? Promise.reject(new Error('provider unavailable'))
          : super.listDirectory(...args);
      }
    }
    const profile = base('1', 'Remote', { right: remote });
    const { controller, settings, notices, applyCalls } = await setup(
      [profile],
      undefined,
      new OfflineRemote(),
    );
    await controller.run('1');
    expect(controller.pendingReview()).toBeUndefined();
    expect(applyCalls).toHaveLength(0);
    const last = settings().syncProfiles[0]?.lastRun;
    expect(last?.outcome).toBe('failed');
    expect(last?.message?.toLowerCase()).toContain('right');
    expect(notices).toHaveLength(1);
  });

  it('reports up to date when the roots match', async () => {
    const same = base('1', 'Same', { left: projects, right: projects });
    const { controller, settings } = await setup([same]);
    await controller.run('1');
    expect(controller.pendingReview()).toBeUndefined();
    expect(settings().syncProfiles[0]?.lastRun?.outcome).toBe('upToDate');
  });
});

describe('scheduled runs', () => {
  const schedule = (confirmation: 'required' | 'autoApplyAdditions') => ({
    enabled: true,
    intervalMinutes: 60,
    confirmation,
  });

  it('prepares an inspectable plan and waits for confirmation by default', async () => {
    const { controller, settings, applyCalls, notices } = await setup([
      base('1', 'Docs', { schedule: schedule('required') }),
    ]);
    await controller.tick();
    const review = controller.pendingReview();
    expect(review?.open).toBe(false);
    expect(review?.trigger).toBe('scheduled');
    expect(applyCalls).toHaveLength(0);
    expect(notices).toHaveLength(1);
    expect(settings().syncProfiles[0]?.lastRun).toMatchObject({
      trigger: 'scheduled',
      outcome: 'planned',
    });
    controller.openReview();
    expect(controller.pendingReview()?.open).toBe(true);
  });

  it('does not fire again before the interval, nor while a review is pending', async () => {
    const { controller, clock } = await setup([
      base('1', 'Docs', { schedule: schedule('required') }),
    ]);
    await controller.tick();
    await controller.dismissReview();
    clock.now = new Date(clock.now.getTime() + 30 * 60_000);
    await controller.tick();
    expect(controller.pendingReview()).toBeUndefined();
    clock.now = new Date(clock.now.getTime() + 31 * 60_000);
    await controller.tick();
    expect(controller.pendingReview()).toBeDefined();
  });

  it('auto-applies only additions and leaves overwrites and deletions for review', async () => {
    const { controller, applyCalls } = await setup([
      base('1', 'Docs', { mode: 'mirrorLeftToRight', schedule: schedule('autoApplyAdditions') }),
    ]);
    await controller.tick();
    for (const call of applyCalls) {
      expect(call.actions.every((action) => action === 'copyLeftToRight')).toBe(true);
    }
    const review = controller.pendingReview();
    expect(review?.plan.items.every((i) => i.action !== 'skip')).toBe(true);
    const autoApplied = new Set(applyCalls.flatMap((c) => c.actions));
    expect(autoApplied.has('deleteRight' as never)).toBe(false);
  });

  it('never fires a profile whose schedule is not enabled', async () => {
    const { controller } = await setup([
      base('1', 'Docs', { schedule: { ...schedule('required'), enabled: false } }),
    ]);
    await controller.tick();
    expect(controller.pendingReview()).toBeUndefined();
  });
});

describe('describeRunError', () => {
  it('names the side and the next step', () => {
    const message = describeRunError(
      new SyncRunError('connectionMissing', 'right', 'provider unavailable'),
    );
    expect(message.toLowerCase()).toContain('right');
    expect(describeRunError(new SyncRunError('planStale', undefined, ''))).not.toBe('');
    expect(describeRunError(new Error('boom'))).toBe('boom');
  });
});
