import type { FileManagerClient } from '../../api/client/file-manager-client';
import type { Location, PaneId, SyncPlanItem, SyncProfile } from '../../models';
import { isPlanStale, operationConflictPolicy } from './sync-profile';

/** Why a profile run could not produce (or apply) a plan; rendered by `describeRunError`. */
export type SyncRunErrorCode =
  | 'connectionMissing'
  | 'rootUnavailable'
  | 'rootNotWritable'
  | 'comparisonFailed'
  | 'comparisonTimeout'
  | 'planStale'
  | 'planUnknown'
  | 'applyFailed';

export class SyncRunError extends Error {
  constructor(
    readonly code: SyncRunErrorCode,
    /** Which root caused it, when one did. */
    readonly side: 'left' | 'right' | undefined,
    detail: string,
  ) {
    super(detail);
    this.name = 'SyncRunError';
  }
}

/** A dry-run plan prepared from freshly compared roots. Nothing has been changed yet. */
export interface PreparedPlan {
  readonly profileId: string;
  readonly comparisonId: string;
  readonly items: readonly SyncPlanItem[];
  readonly generatedAtMs: number;
  /** Entries the comparison could not read (permissions, races); the plan may be incomplete. */
  readonly warningsCount: number;
}

export interface RunContext {
  readonly client: FileManagerClient;
  readonly workspaceId: string;
  /** A pane id used only to satisfy the listing request; no pane is navigated. */
  readonly paneId: PaneId;
  readonly now: () => number;
  readonly sleep: (ms: number) => Promise<void>;
  readonly pollIntervalMs?: number;
  readonly timeoutMs?: number;
}

const DEFAULT_POLL_MS = 100;
const DEFAULT_TIMEOUT_MS = 10 * 60 * 1000;

function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function errorCode(error: unknown): string {
  const code = (error as { code?: unknown } | null)?.code;
  return typeof code === 'string' ? code.toLowerCase() : '';
}

/** Whether a failure means the provider or its connection is not available. */
function isConnectionProblem(error: unknown): boolean {
  const text = `${errorCode(error)} ${errorText(error)}`.toLowerCase();
  return /provider|connection|credential|unauthori[sz]ed|not connected/u.test(text);
}

/** Which roots the profile's direction writes into. */
export function writtenSides(profile: SyncProfile): readonly ('left' | 'right')[] {
  switch (profile.mode) {
    case 'mirrorLeftToRight':
      return ['right'];
    case 'mirrorRightToLeft':
      return ['left'];
    case 'twoWayUpdate':
      return ['left', 'right'];
  }
}

let requestSequence = 0;

/** Lists a root once to prove it is reachable and, when the plan writes there, still writable. */
async function preflight(
  context: RunContext,
  side: 'left' | 'right',
  location: Location,
  needsWrite: boolean,
): Promise<void> {
  requestSequence += 1;
  try {
    const snapshot = await context.client.listDirectory({
      workspaceId: context.workspaceId,
      paneId: context.paneId,
      requestId: `sync-profile-preflight-${requestSequence}`,
      location,
    });
    if (needsWrite && !snapshot.writable) {
      throw new SyncRunError('rootNotWritable', side, 'The location is read-only.');
    }
  } catch (error) {
    if (error instanceof SyncRunError) throw error;
    throw new SyncRunError(
      isConnectionProblem(error) ? 'connectionMissing' : 'rootUnavailable',
      side,
      errorText(error),
    );
  }
}

/**
 * Refreshes both roots (a new comparison - never a cached one), waits for it to finish and
 * generates the dry-run plan with the profile's direction and filters. Mutates nothing.
 */
export async function preparePlan(
  context: RunContext,
  profile: SyncProfile,
): Promise<PreparedPlan> {
  const written = writtenSides(profile);
  await preflight(context, 'left', profile.left, written.includes('left'));
  await preflight(context, 'right', profile.right, written.includes('right'));

  let comparisonId: string;
  try {
    ({ comparisonId } = await context.client.startComparison({
      workspaceId: context.workspaceId,
      left: profile.left,
      right: profile.right,
      criteria: profile.criteria,
      showHidden: profile.showHidden,
    }));
  } catch (error) {
    throw new SyncRunError(
      isConnectionProblem(error) ? 'connectionMissing' : 'comparisonFailed',
      undefined,
      errorText(error),
    );
  }

  const deadline = context.now() + (context.timeoutMs ?? DEFAULT_TIMEOUT_MS);
  let warningsCount = 0;
  for (;;) {
    const page = await context.client
      .getComparison(comparisonId, { offset: 0, limit: 1 })
      .catch((error: unknown) => {
        throw new SyncRunError('comparisonFailed', undefined, errorText(error));
      });
    warningsCount = page.warningsCount;
    if (page.isComplete) break;
    if (context.now() >= deadline) {
      await context.client.cancelComparison(comparisonId).catch(() => undefined);
      throw new SyncRunError('comparisonTimeout', undefined, 'The comparison took too long.');
    }
    await context.sleep(context.pollIntervalMs ?? DEFAULT_POLL_MS);
  }

  const plan = await context.client
    .generateSyncPlan(comparisonId, {
      mode: profile.mode,
      include: profile.include,
      exclude: profile.exclude,
    })
    .catch((error: unknown) => {
      throw new SyncRunError('comparisonFailed', undefined, errorText(error));
    });
  return {
    profileId: profile.id,
    comparisonId,
    items: plan.items,
    generatedAtMs: context.now(),
    warningsCount,
  };
}

/**
 * Applies reviewed rows of a prepared plan through the operation engine. Refuses plans that are
 * too old or whose comparison the backend no longer holds, so a stale preview is never executed.
 */
export async function applyPreparedPlan(
  context: RunContext,
  profile: SyncProfile,
  plan: PreparedPlan,
  items: readonly SyncPlanItem[],
): Promise<number> {
  if (isPlanStale(plan.generatedAtMs, context.now())) {
    throw new SyncRunError('planStale', undefined, 'The plan is out of date.');
  }
  const rows = items.filter((item) => item.action !== 'skip');
  if (rows.length === 0) return 0;
  try {
    const result = await context.client.applySyncPlan(plan.comparisonId, {
      items: rows,
      conflictPolicy: operationConflictPolicy(profile.conflictPolicy),
    });
    return result.operationIds.length;
  } catch (error) {
    const text = `${errorCode(error)} ${errorText(error)}`.toLowerCase();
    if (/not.?found|404/u.test(text)) {
      throw new SyncRunError('planUnknown', undefined, errorText(error));
    }
    throw new SyncRunError('applyFailed', undefined, errorText(error));
  }
}
