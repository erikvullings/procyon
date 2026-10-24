import { describe, expect, it } from 'vitest';
import type { FileManagerClient } from '../../api/client/file-manager-client';
import { MockFileManagerClient } from '../../api/client/mock-file-manager-client';
import type { Location, SyncPlan, SyncProfile } from '../../models';
import { newSyncProfile } from './sync-profile';
import {
  applyPreparedPlan,
  preparePlan,
  type RunContext,
  SyncRunError,
  type SyncRunErrorCode,
  writtenSides,
} from './sync-profile-runner';

const docs: Location = { providerId: 'mock', uri: 'mock:///Documents' };
const projects: Location = { providerId: 'mock', uri: 'mock:///Documents/Projects' };
const remote: Location = { providerId: 'sftp', uri: 'sftp://me@nas/share' };

function context(
  client: FileManagerClient,
  clock = { now: 1_000 },
): RunContext & { clock: { now: number } } {
  return {
    client,
    workspaceId: 'workspace-1',
    paneId: 'pane-left',
    now: () => clock.now,
    sleep: (ms) => {
      clock.now += ms;
      return Promise.resolve();
    },
    clock,
  };
}

async function failureCode(
  promise: Promise<unknown>,
): Promise<[SyncRunErrorCode, string | undefined]> {
  try {
    await promise;
  } catch (error) {
    if (error instanceof SyncRunError) return [error.code, error.side];
    throw error;
  }
  throw new Error('expected a SyncRunError');
}

/** A client whose listing/comparison behaviour a test scripts per location. */
class ScriptedClient extends MockFileManagerClient {
  readonly started: { left: string; right: string }[] = [];
  unavailable = new Map<string, Error>();
  readOnly = new Set<string>();
  incompletePolls = 0;
  generated: { include?: readonly string[]; exclude?: readonly string[] }[] = [];

  override listDirectory(
    ...args: Parameters<MockFileManagerClient['listDirectory']>
  ): ReturnType<MockFileManagerClient['listDirectory']> {
    const failure = this.unavailable.get(args[0].location.uri);
    if (failure !== undefined) return Promise.reject(failure);
    return super.listDirectory(...args).then((snapshot) => ({
      ...snapshot,
      writable: !this.readOnly.has(args[0].location.uri),
    }));
  }

  override startComparison(
    ...args: Parameters<MockFileManagerClient['startComparison']>
  ): ReturnType<MockFileManagerClient['startComparison']> {
    this.started.push({ left: args[0].left.uri, right: args[0].right.uri });
    return super.startComparison(...args);
  }

  override getComparison(
    ...args: Parameters<MockFileManagerClient['getComparison']>
  ): ReturnType<MockFileManagerClient['getComparison']> {
    return super.getComparison(...args).then((page) => {
      if (this.incompletePolls > 0) {
        this.incompletePolls -= 1;
        return { ...page, isComplete: false };
      }
      return { ...page, warningsCount: 2 };
    });
  }

  override generateSyncPlan(
    ...args: Parameters<MockFileManagerClient['generateSyncPlan']>
  ): Promise<SyncPlan> {
    this.generated.push({ include: args[1].include ?? [], exclude: args[1].exclude ?? [] });
    return super.generateSyncPlan(...args);
  }
}

const profile = (extra: Partial<SyncProfile> = {}): SyncProfile => ({
  ...newSyncProfile('p1', 'Docs', docs, projects),
  ...extra,
});

describe('preparePlan', () => {
  it('refreshes both roots with a new comparison and returns a dry-run plan', async () => {
    const client = new ScriptedClient();
    client.incompletePolls = 2;
    const ctx = context(client);
    const first = await preparePlan(ctx, profile({ include: ['*.md'], exclude: ['tmp'] }));
    const second = await preparePlan(ctx, profile());
    expect(client.started).toHaveLength(2);
    expect(first.comparisonId).not.toBe(second.comparisonId);
    expect(first.warningsCount).toBe(2);
    expect(client.generated[0]).toEqual({ include: ['*.md'], exclude: ['tmp'] });
    expect(ctx.clock.now).toBeGreaterThan(1_000); // it polled until complete
    expect(first.generatedAtMs).toBeGreaterThan(1_000);
  });

  it('reports a missing connection for a remote root and names the side', async () => {
    const client = new ScriptedClient();
    client.unavailable.set(remote.uri, new Error('provider unavailable'));
    const run = preparePlan(context(client), profile({ right: remote }));
    expect(await failureCode(run)).toEqual(['connectionMissing', 'right']);
    expect(client.started).toHaveLength(0);
  });

  it('reports an unavailable local location without starting a comparison', async () => {
    const client = new ScriptedClient();
    client.unavailable.set(docs.uri, new Error('No such directory'));
    expect(await failureCode(preparePlan(context(client), profile()))).toEqual([
      'rootUnavailable',
      'left',
    ]);
    expect(client.started).toHaveLength(0);
  });

  it('rejects a destination whose capabilities changed to read-only, only when written to', async () => {
    const client = new ScriptedClient();
    client.readOnly.add(projects.uri);
    const mirrorRight = profile({ mode: 'mirrorLeftToRight' });
    expect(await failureCode(preparePlan(context(client), mirrorRight))).toEqual([
      'rootNotWritable',
      'right',
    ]);
    // Mirroring right-to-left never writes into the (read-only) right side.
    const mirrorLeft = profile({ mode: 'mirrorRightToLeft' });
    await expect(preparePlan(context(client), mirrorLeft)).resolves.toBeDefined();
    expect(writtenSides(profile({ mode: 'twoWayUpdate' }))).toEqual(['left', 'right']);
  });

  it('times out and cancels a comparison that never completes', async () => {
    const client = new ScriptedClient();
    client.incompletePolls = Number.MAX_SAFE_INTEGER;
    const ctx = { ...context(client), timeoutMs: 500 };
    expect(await failureCode(preparePlan(ctx, profile()))).toEqual([
      'comparisonTimeout',
      undefined,
    ]);
  });
});

describe('applyPreparedPlan', () => {
  it('applies reviewed rows with the profile conflict policy and skips skip rows', async () => {
    const client = new ScriptedClient();
    const ctx = context(client);
    const prepared = await preparePlan(ctx, profile({ conflictPolicy: 'keepBoth' }));
    const seen: { policy?: string; count: number }[] = [];
    const original = client.applySyncPlan.bind(client);
    client.applySyncPlan = (id, request, signal) => {
      seen.push({
        ...(request.conflictPolicy === undefined ? {} : { policy: request.conflictPolicy }),
        count: request.items.length,
      });
      return original(id, request, signal);
    };
    const items = [
      ...prepared.items,
      ...prepared.items.map((i) => ({ ...i, action: 'skip' as const })),
    ];
    const applied = await applyPreparedPlan(
      ctx,
      profile({ conflictPolicy: 'keepBoth' }),
      prepared,
      items,
    );
    expect(applied).toBe(prepared.items.filter((i) => i.action !== 'skip').length);
    expect(seen[0]?.policy).toBe('renameNew');
    expect(seen[0]?.count).toBe(applied);
  });

  it('refuses a stale plan and an unknown comparison', async () => {
    const client = new ScriptedClient();
    const ctx = context(client);
    const prepared = await preparePlan(ctx, profile());
    ctx.clock.now += 60 * 60 * 1000;
    expect(
      (await failureCode(applyPreparedPlan(ctx, profile(), prepared, prepared.items)))[0],
    ).toBe('planStale');
    ctx.clock.now = prepared.generatedAtMs;
    const unknown = { ...prepared, comparisonId: 'gone' };
    const row = prepared.items.find((item) => item.action !== 'skip');
    expect(row).toBeDefined();
    if (row === undefined) return;
    expect((await failureCode(applyPreparedPlan(ctx, profile(), unknown, [row])))[0]).toBe(
      'planUnknown',
    );
  });

  it('applies nothing when every row is skip', async () => {
    const client = new ScriptedClient();
    const ctx = context(client);
    const prepared = await preparePlan(ctx, profile());
    const skipped = prepared.items.map((item) => ({ ...item, action: 'skip' as const }));
    expect(await applyPreparedPlan(ctx, profile(), prepared, skipped)).toBe(0);
  });
});
