import type { FileManagerClient } from '../../api/client/file-manager-client';
import { t } from '../../i18n';
import type {
  PaneId,
  Settings,
  SyncPlanItem,
  SyncProfile,
  SyncRunOutcome,
  SyncRunTrigger,
} from '../../models';
import {
  dueProfiles,
  duplicateSyncProfile,
  type ProfileValidationIssue,
  removeSyncProfile,
  runSummary,
  splitScheduledPlan,
  upsertSyncProfile,
  validateSyncProfile,
  withLastRun,
} from './sync-profile';
import {
  applyPreparedPlan,
  type PreparedPlan,
  preparePlan,
  type RunContext,
  SyncRunError,
} from './sync-profile-runner';

/** A prepared plan awaiting a person's review. */
export interface PendingReview {
  readonly profile: SyncProfile;
  readonly plan: PreparedPlan;
  readonly trigger: SyncRunTrigger;
  /** Whether the review dialog is showing; scheduled plans wait until the user opens them. */
  readonly open: boolean;
  readonly applying: boolean;
  readonly error?: string;
}

export interface SyncProfileControllerContext {
  getClient(): FileManagerClient;
  getSettings(): Settings | undefined;
  updateSettings(update: (settings: Settings) => Settings): Promise<void>;
  /** Workspace and a pane id for listing requests; undefined while no workspace is open. */
  getRunTarget(): { readonly workspaceId: string; readonly paneId: PaneId } | undefined;
  notify(message: string): void;
  redraw(): void;
  newId(): string;
  now(): Date;
  sleep(ms: number): Promise<void>;
}

export interface SyncProfileController {
  profiles(): readonly SyncProfile[];
  isRunning(id: string): boolean;
  pendingReview(): PendingReview | undefined;
  /** Validates and persists a profile; returns the issues when it was rejected. */
  save(profile: SyncProfile): Promise<readonly ProfileValidationIssue[]>;
  duplicate(id: string): Promise<void>;
  remove(id: string): Promise<void>;
  /** Prepares a dry-run plan; manual runs open the review immediately. */
  run(id: string, trigger?: SyncRunTrigger): Promise<void>;
  openReview(): void;
  confirmReview(items: readonly SyncPlanItem[]): Promise<void>;
  dismissReview(): Promise<void>;
  /** Starts the first due scheduled profile (one at a time, none while a review is pending). */
  tick(): Promise<void>;
}

/** Actionable text for a failed run; names the side and what to do about it. */
export function describeRunError(error: unknown): string {
  if (!(error instanceof SyncRunError)) {
    return error instanceof Error ? error.message : String(error);
  }
  const side =
    error.side === undefined
      ? ''
      : t('syncProfiles', error.side === 'left' ? 'sideLeft' : 'sideRight');
  switch (error.code) {
    case 'connectionMissing':
      return t('syncProfiles', 'errConnectionMissing', { side, detail: error.message });
    case 'rootUnavailable':
      return t('syncProfiles', 'errRootUnavailable', { side, detail: error.message });
    case 'rootNotWritable':
      return t('syncProfiles', 'errRootNotWritable', { side });
    case 'comparisonTimeout':
      return t('syncProfiles', 'errTimeout');
    case 'planStale':
      return t('syncProfiles', 'errPlanStale');
    case 'planUnknown':
      return t('syncProfiles', 'errPlanUnknown');
    case 'applyFailed':
      return t('syncProfiles', 'errApplyFailed', { detail: error.message });
    case 'comparisonFailed':
      return t('syncProfiles', 'errComparisonFailed', { detail: error.message });
  }
}

export function createSyncProfileController(
  context: SyncProfileControllerContext,
): SyncProfileController {
  const running = new Set<string>();
  let review: PendingReview | undefined;

  const profiles = (): readonly SyncProfile[] => context.getSettings()?.syncProfiles ?? [];

  async function persist(
    change: (current: readonly SyncProfile[]) => readonly SyncProfile[],
  ): Promise<void> {
    await context.updateSettings((settings) => ({
      ...settings,
      syncProfiles: change(settings.syncProfiles),
    }));
  }

  function runContext(): RunContext | undefined {
    const target = context.getRunTarget();
    if (target === undefined) return undefined;
    return {
      client: context.getClient(),
      workspaceId: target.workspaceId,
      paneId: target.paneId,
      now: () => context.now().getTime(),
      sleep: (ms) => context.sleep(ms),
    };
  }

  async function record(
    profileId: string,
    trigger: SyncRunTrigger,
    outcome: SyncRunOutcome,
    plannedItems: number,
    appliedItems = 0,
    message?: string,
  ): Promise<void> {
    const summary = runSummary({
      now: context.now(),
      trigger,
      outcome,
      plannedItems,
      appliedItems,
      ...(message === undefined ? {} : { message }),
    });
    await persist((current) => withLastRun(current, profileId, summary)).catch(() => undefined);
  }

  async function applyRows(
    profile: SyncProfile,
    plan: PreparedPlan,
    items: readonly SyncPlanItem[],
  ): Promise<number> {
    const ctx = runContext();
    if (ctx === undefined) throw new Error(t('syncProfiles', 'errNoWorkspace'));
    return applyPreparedPlan(ctx, profile, plan, items);
  }

  const controller: SyncProfileController = {
    profiles,
    isRunning: (id) => running.has(id),
    pendingReview: () => review,

    async save(profile) {
      const issues = validateSyncProfile(profile, profiles());
      if (issues.length > 0) return issues;
      const normalized: SyncProfile = { ...profile, name: profile.name.trim() };
      await persist((current) => upsertSyncProfile(current, normalized));
      return [];
    },

    async duplicate(id) {
      const copy = duplicateSyncProfile(profiles(), id, context.newId());
      if (copy !== undefined) await persist((current) => [...current, copy]);
    },

    async remove(id) {
      if (review?.profile.id === id) review = undefined;
      await persist((current) => removeSyncProfile(current, id));
    },

    async run(id, trigger = 'manual') {
      const profile = profiles().find((candidate) => candidate.id === id);
      if (profile === undefined || running.has(id) || review !== undefined) return;
      const ctx = runContext();
      if (ctx === undefined) return;
      running.add(id);
      context.redraw();
      try {
        const plan = await preparePlan(ctx, profile);
        const actionable = plan.items.filter((item) => item.action !== 'skip');
        if (actionable.length === 0) {
          await record(id, trigger, 'upToDate', plan.items.length);
          context.notify(t('syncProfiles', 'upToDate', { name: profile.name }));
          return;
        }
        let pending = actionable;
        let applied = 0;
        if (trigger === 'scheduled') {
          const split = splitScheduledPlan(profile.schedule, plan.items);
          pending = [...split.review];
          if (split.automatic.length > 0) {
            applied = await applyRows(profile, plan, split.automatic);
          }
        }
        if (pending.length === 0) {
          await record(id, trigger, 'applied', actionable.length, applied);
          context.notify(t('syncProfiles', 'autoApplied', { name: profile.name, count: applied }));
          return;
        }
        review = {
          profile,
          plan: { ...plan, items: plan.items.filter((item) => pending.includes(item)) },
          trigger,
          open: trigger === 'manual',
          applying: false,
        };
        await record(id, trigger, 'planned', actionable.length, applied);
        if (trigger === 'scheduled') {
          context.notify(t('syncProfiles', 'reviewReady', { name: profile.name }));
        }
      } catch (error) {
        const message = describeRunError(error);
        await record(id, trigger, 'failed', 0, 0, message);
        context.notify(t('syncProfiles', 'runFailed', { name: profile.name, message }));
      } finally {
        running.delete(id);
        context.redraw();
      }
    },

    openReview() {
      if (review !== undefined) {
        review = { ...review, open: true };
        context.redraw();
      }
    },

    async confirmReview(items) {
      const current = review;
      if (current === undefined || current.applying) return;
      review = { ...current, applying: true };
      context.redraw();
      try {
        const applied = await applyRows(current.profile, current.plan, items);
        review = undefined;
        await record(
          current.profile.id,
          current.trigger,
          'applied',
          current.plan.items.length,
          applied,
        );
        context.notify(
          t('syncProfiles', 'applied', { name: current.profile.name, count: applied }),
        );
      } catch (error) {
        review = { ...current, applying: false, error: describeRunError(error) };
      }
      context.redraw();
    },

    async dismissReview() {
      const current = review;
      if (current === undefined) return;
      review = undefined;
      await record(current.profile.id, current.trigger, 'dismissed', current.plan.items.length);
      context.redraw();
    },

    async tick() {
      if (review !== undefined || running.size > 0) return;
      const [next] = dueProfiles(profiles(), context.now());
      if (next !== undefined) await controller.run(next.id, 'scheduled');
    },
  };
  return controller;
}
