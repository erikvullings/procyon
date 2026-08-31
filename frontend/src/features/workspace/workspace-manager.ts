import type { WorkspaceId, WorkspaceSummary } from '../../models';

/** Deterministic, name-ordered listing for the workspace switcher (ties break on id). Excludes
 * ephemeral (per-window) workspaces - only named/template workspaces are ever switchable, and an
 * ephemeral workspace belongs to a specific window, not to a name a user can pick from a list
 * (ephemeral per-window workspaces spec follow-up). */
export function sortWorkspaceSummaries(
  summaries: readonly WorkspaceSummary[],
): readonly WorkspaceSummary[] {
  return summaries
    .filter((summary) => !summary.ephemeral)
    .sort(
      (a, b) =>
        a.name.localeCompare(b.name, undefined, { sensitivity: 'base' }) ||
        a.id.localeCompare(b.id),
    );
}

/**
 * Chooses the workspace that should become active once the current one is no
 * longer valid (deleted locally or by another session). `undefined` tells the
 * caller no persisted workspace remains, so it must create a fresh default.
 * Excludes ephemeral workspaces, same reasoning as `sortWorkspaceSummaries` - recovery must never
 * silently open another window's private ephemeral session.
 */
export function firstAvailableWorkspaceId(
  summaries: readonly WorkspaceSummary[],
): WorkspaceId | undefined {
  return summaries.find((summary) => !summary.ephemeral)?.id;
}
