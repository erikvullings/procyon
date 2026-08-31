import { describe, expect, it } from 'vitest';

import type { WorkspaceSummary } from '../../models';
import { firstAvailableWorkspaceId, sortWorkspaceSummaries } from './workspace-manager';

function summary(id: string, name: string, revision = 1, ephemeral = false): WorkspaceSummary {
  return { id, name, revision, ephemeral, updatedAt: '2026-01-01T00:00:00.000Z' };
}

describe('sortWorkspaceSummaries', () => {
  it('orders summaries by name, case-insensitively', () => {
    const summaries = [summary('c', 'Charlie'), summary('a', 'alpha'), summary('b', 'Bravo')];

    expect(sortWorkspaceSummaries(summaries).map((s) => s.id)).toEqual(['a', 'b', 'c']);
  });

  it('breaks name ties by id for a stable order', () => {
    const summaries = [summary('b', 'Same'), summary('a', 'Same')];

    expect(sortWorkspaceSummaries(summaries).map((s) => s.id)).toEqual(['a', 'b']);
  });

  it('does not mutate the input array', () => {
    const summaries = [summary('b', 'Bravo'), summary('a', 'alpha')];
    const original = [...summaries];

    sortWorkspaceSummaries(summaries);

    expect(summaries).toEqual(original);
  });

  it('returns an empty array unchanged', () => {
    expect(sortWorkspaceSummaries([])).toEqual([]);
  });

  it('excludes ephemeral (per-window) workspaces', () => {
    const summaries = [
      summary('a', 'alpha'),
      summary('b', 'Bravo', 1, true),
      summary('c', 'Charlie'),
    ];

    expect(sortWorkspaceSummaries(summaries).map((s) => s.id)).toEqual(['a', 'c']);
  });
});

describe('firstAvailableWorkspaceId', () => {
  it('returns the first summary id when at least one workspace remains', () => {
    const summaries = [summary('a', 'alpha'), summary('b', 'Bravo')];

    expect(firstAvailableWorkspaceId(summaries)).toBe('a');
  });

  it('returns undefined when no workspaces remain, signalling the caller must create one', () => {
    expect(firstAvailableWorkspaceId([])).toBeUndefined();
  });

  it("skips ephemeral workspaces - recovery must never open another window's private session", () => {
    const summaries = [summary('a', 'alpha', 1, true), summary('b', 'Bravo')];

    expect(firstAvailableWorkspaceId(summaries)).toBe('b');
  });
});
