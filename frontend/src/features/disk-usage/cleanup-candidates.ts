import type { DiskUsageCleanupCandidate, Location } from '../../models';

/** Replaces the expanded subtree's candidates with the rescan's, keeping all others. */
export function mergeCleanupCandidates(
  base: readonly DiskUsageCleanupCandidate[],
  rescanned: readonly DiskUsageCleanupCandidate[],
  expanded: Location,
): DiskUsageCleanupCandidate[] {
  const prefix = expanded.uri.endsWith('/') ? expanded.uri : `${expanded.uri}/`;
  const outside = base.filter(
    (candidate) =>
      candidate.location.uri !== expanded.uri && !candidate.location.uri.startsWith(prefix),
  );
  return [...outside, ...rescanned].sort((left, right) => right.physicalBytes - left.physicalBytes);
}
