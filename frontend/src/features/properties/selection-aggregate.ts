import type { EntrySummary } from '../../models';

/**
 * Aggregate totals for a multi-selection (0097's aggregate-computation approach, applied to a
 * selection instead of a whole directory listing): total size, item count, and a folder/file
 * breakdown, for the Properties dialog's multi-selection view.
 */
export interface SelectionAggregate {
  readonly itemCount: number;
  readonly fileCount: number;
  readonly folderCount: number;
  readonly totalSize: number;
}

/** Sums size and partitions by kind across the given entries, mirroring the backend's
 * `aggregate_totals` (`crates/fm-application/src/directory.rs`) and the frontend's
 * `listingSummary` (`frontend/src/features/panes/pane.ts`) kind-partitioning shape. */
export function computeSelectionAggregate(entries: readonly EntrySummary[]): SelectionAggregate {
  let fileCount = 0;
  let folderCount = 0;
  let totalSize = 0;
  for (const entry of entries) {
    if (entry.kind === 'directory') {
      folderCount += 1;
    } else {
      fileCount += 1;
    }
    totalSize += entry.size ?? 0;
  }
  return {
    itemCount: entries.length,
    fileCount,
    folderCount,
    totalSize,
  };
}
