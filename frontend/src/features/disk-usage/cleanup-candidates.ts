import type { DiskUsageCleanupCandidate, Location } from '../../models';

/**
 * Replaces candidates strictly inside the expanded folder with the rescan's, keeping all others.
 * The expanded folder itself is kept: a rescan rooted there never reports its own root.
 */
export function mergeCleanupCandidates(
  base: readonly DiskUsageCleanupCandidate[],
  rescanned: readonly DiskUsageCleanupCandidate[],
  expanded: Location,
): DiskUsageCleanupCandidate[] {
  const prefix = expanded.uri.endsWith('/') ? expanded.uri : `${expanded.uri}/`;
  const outside = base.filter((candidate) => !candidate.location.uri.startsWith(prefix));
  return [...outside, ...rescanned].sort((left, right) => right.physicalBytes - left.physicalBytes);
}
