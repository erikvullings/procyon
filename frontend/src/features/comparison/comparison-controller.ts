import type { FileManagerClient } from '../../api/client/file-manager-client';
import { t } from '../../i18n';
import type { ComparisonCriteria, ComparisonEntry, WorkspaceProjection } from '../../models';
import {
  type ComparisonState,
  withComparisonBatch,
  withComparisonCleared,
  withComparisonError,
  withComparisonStarted,
  withDifferencesOnly,
} from './comparison-state';

/** Context required by ComparisonController for state access and dependencies. */
export interface ComparisonControllerContext {
  getState(): ComparisonState;
  setState(next: ComparisonState): void;
  getWorkspace(): WorkspaceProjection | undefined;
  getClient(): FileManagerClient;
  redraw(): void;
}

/** Controller interface for directory-comparison operations (task 0075). */
export interface ComparisonController {
  /** Compares the first two panes' current directories (the dual-pane MVP layout). */
  startComparison(criteria: ComparisonCriteria): void;

  /** Cancels the active comparison, if any, and clears its overlay. */
  cancelComparison(): void;

  /** Toggles whether panes show only non-identical entries while a comparison is active. */
  setDifferencesOnly(value: boolean): void;

  /** Applies one streamed `comparison.resultsBatch` event to the active comparison's state. */
  handleResultsBatch(
    comparisonId: string,
    entries: readonly ComparisonEntry[],
    isComplete: boolean,
    warningsCount: number,
  ): void;
}

/** Factory function to create a ComparisonController. */
export function createComparisonController(
  context: ComparisonControllerContext,
): ComparisonController {
  return {
    startComparison(criteria: ComparisonCriteria): void {
      const workspace = context.getWorkspace();
      if (workspace === undefined) return;
      const [leftPaneId, rightPaneId] = workspace.paneOrder;
      if (leftPaneId === undefined || rightPaneId === undefined) {
        context.setState(
          withComparisonError(context.getState(), 'Comparing directories needs two open panes.'),
        );
        context.redraw();
        return;
      }
      const leftPane = workspace.panesById[leftPaneId];
      const rightPane = workspace.panesById[rightPaneId];
      const leftRoot = leftPane?.tabsById[leftPane.activeTabId]?.location;
      const rightRoot = rightPane?.tabsById[rightPane.activeTabId]?.location;
      if (leftRoot === undefined || rightRoot === undefined) return;

      const previousId = context.getState().comparisonId;
      if (previousId !== undefined) {
        void context
          .getClient()
          .cancelComparison(previousId)
          .catch(() => undefined);
      }

      void context
        .getClient()
        .startComparison({
          workspaceId: workspace.id,
          left: leftRoot,
          right: rightRoot,
          criteria,
        })
        .then((result) => {
          context.setState(
            withComparisonStarted({
              comparisonId: result.comparisonId,
              criteria,
              leftRoot,
              rightRoot,
              leftPaneId,
              rightPaneId,
            }),
          );
          context.redraw();
        })
        .catch((error: unknown) => {
          context.setState(
            withComparisonError(
              context.getState(),
              error instanceof Error ? error.message : t('comparison', 'unableToStart'),
            ),
          );
          context.redraw();
        });
    },

    cancelComparison(): void {
      const { comparisonId } = context.getState();
      if (comparisonId === undefined) return;
      void context
        .getClient()
        .cancelComparison(comparisonId)
        .catch(() => undefined);
      context.setState(withComparisonCleared());
      context.redraw();
    },

    setDifferencesOnly(value: boolean): void {
      context.setState(withDifferencesOnly(context.getState(), value));
      context.redraw();
    },

    handleResultsBatch(
      comparisonId: string,
      entries: readonly ComparisonEntry[],
      isComplete: boolean,
      warningsCount: number,
    ): void {
      context.setState(
        withComparisonBatch(context.getState(), comparisonId, entries, isComplete, warningsCount),
      );
      context.redraw();
    },
  };
}
