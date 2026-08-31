import type { FileManagerClient } from '../../api/client/file-manager-client';
import { t } from '../../i18n';
import type {
  ChecksumAlgorithm,
  ChecksumEntry,
  DuplicateGroup,
  EntrySummary,
  Location,
  WorkspaceProjection,
} from '../../models';
import {
  type ChecksumState,
  type DuplicateState,
  selectedLocations,
  withChecksumBatch,
  withChecksumCleared,
  withChecksumError,
  withChecksumJobStarted,
  withChecksumSaved,
  withDuplicateCleared,
  withDuplicateError,
  withDuplicateResults,
  withDuplicateScanStarted,
  withDuplicateSelectionCleared,
  withDuplicateSelectionToggled,
  withVerificationReport,
} from './checksum-state';

/** Context required by ChecksumController for state access and dependencies. */
export interface ChecksumControllerContext {
  getChecksumState(): ChecksumState;
  setChecksumState(next: ChecksumState): void;
  getDuplicateState(): DuplicateState;
  setDuplicateState(next: DuplicateState): void;
  getWorkspace(): WorkspaceProjection | undefined;
  getClient(): FileManagerClient;
  /** The entries currently selected in the active pane. */
  getSelectedEntries(): readonly EntrySummary[];
  /** The active pane's current directory, used as the duplicate-scan root. */
  getActiveLocation(): Location | undefined;
  /**
   * Hands locations to the application's existing delete-with-confirmation
   * flow. Duplicate review deliberately does not implement its own deletion
   * (spec §35, task 0077): it reuses the audited path every other delete goes
   * through, so confirmation, conflict handling and undo behave identically.
   */
  requestDelete(locations: readonly Location[]): void;
  redraw(): void;
}

/** Controller interface for checksum and duplicate-detection features (task 0077). */
export interface ChecksumController {
  /** Starts a checksum job over the active pane's selection. */
  calculateChecksums(algorithms: readonly ChecksumAlgorithm[]): void;

  /** Cancels the active checksum job, if any, and clears its panel. */
  cancelChecksums(): void;

  /** Clears the checksum panel without cancelling anything. */
  closeChecksums(): void;

  /** Copies the current results to the clipboard as checksum-file text. */
  copyChecksums(algorithm: ChecksumAlgorithm): Promise<string | undefined>;

  /** Renders the current results as checksum-file text for saving. */
  renderChecksumFile(
    algorithm: ChecksumAlgorithm,
  ): Promise<{ suggestedName: string; content: string } | undefined>;

  /** The filename a save should default to, e.g. `checksums.sha256`. */
  suggestedFileName(algorithm: ChecksumAlgorithm): string;

  /**
   * Writes the results to `fileName` in the active pane's directory.
   *
   * Resolves to the saved location, or `undefined` if the save could not be
   * attempted or failed (in which case the error is on the state).
   */
  saveChecksumFile(
    algorithm: ChecksumAlgorithm,
    fileName: string,
    options?: { overwrite?: boolean },
  ): Promise<Location | undefined>;

  /** Verifies the current results against an existing checksum file's text. */
  verifyAgainst(content: string): void;

  /** Applies one streamed `checksum.resultsBatch` event. */
  handleChecksumBatch(
    jobId: string,
    entries: readonly ChecksumEntry[],
    isComplete: boolean,
    isCancelled: boolean,
  ): void;

  /** Starts a duplicate scan rooted at the active pane's directory. */
  findDuplicates(): void;

  /** Cancels the active duplicate scan and clears its panel. */
  cancelDuplicateScan(): void;

  /** Clears the duplicate panel without cancelling anything. */
  closeDuplicates(): void;

  /** Ticks or unticks one duplicate path for deletion. */
  toggleDuplicateSelection(uri: string): void;

  /** Sends every ticked duplicate through the normal delete-with-confirmation flow. */
  deleteSelectedDuplicates(): void;

  /** Applies the terminal `duplicates.resultsReady` event. */
  handleDuplicateResults(
    scanId: string,
    groups: readonly DuplicateGroup[],
    isCancelled: boolean,
    warningsCount: number,
  ): void;
}

function message(error: unknown, fallback: string): string {
  return error instanceof Error ? error.message : fallback;
}

/** Factory function to create a ChecksumController. */
export function createChecksumController(context: ChecksumControllerContext): ChecksumController {
  return {
    calculateChecksums(algorithms: readonly ChecksumAlgorithm[]): void {
      const workspace = context.getWorkspace();
      if (workspace === undefined) return;
      const entries = context.getSelectedEntries().filter((entry) => entry.kind === 'file');
      if (entries.length === 0) {
        context.setChecksumState(
          withChecksumError(context.getChecksumState(), t('checksums', 'selectFilesToCalculate')),
        );
        context.redraw();
        return;
      }
      if (algorithms.length === 0) return;

      const previousId = context.getChecksumState().jobId;
      if (previousId !== undefined) {
        void context
          .getClient()
          .cancelChecksums(previousId)
          .catch(() => undefined);
      }

      void context
        .getClient()
        .startChecksums({
          workspaceId: workspace.id,
          entries: entries.map((entry) => entry.location),
          algorithms: [...algorithms],
        })
        .then((result) => {
          context.setChecksumState(
            withChecksumJobStarted(result.jobId, algorithms, entries.length),
          );
          context.redraw();
        })
        .catch((error: unknown) => {
          context.setChecksumState(
            withChecksumError(
              context.getChecksumState(),
              message(error, t('checksums', 'unableToStartCalculation')),
            ),
          );
          context.redraw();
        });
    },

    cancelChecksums(): void {
      const { jobId } = context.getChecksumState();
      if (jobId === undefined) return;
      void context
        .getClient()
        .cancelChecksums(jobId)
        .catch(() => undefined);
      context.setChecksumState(withChecksumCleared());
      context.redraw();
    },

    closeChecksums(): void {
      context.setChecksumState(withChecksumCleared());
      context.redraw();
    },

    async copyChecksums(algorithm: ChecksumAlgorithm): Promise<string | undefined> {
      const file = await this.renderChecksumFile(algorithm);
      return file?.content;
    },

    async renderChecksumFile(
      algorithm: ChecksumAlgorithm,
    ): Promise<{ suggestedName: string; content: string } | undefined> {
      const { jobId } = context.getChecksumState();
      if (jobId === undefined) return undefined;
      try {
        return await context.getClient().renderChecksumFile(jobId, algorithm);
      } catch (error: unknown) {
        context.setChecksumState(
          withChecksumError(
            context.getChecksumState(),
            message(error, t('checksums', 'unableToRenderFile')),
          ),
        );
        context.redraw();
        return undefined;
      }
    },

    suggestedFileName(algorithm: ChecksumAlgorithm): string {
      return `checksums.${algorithm}`;
    },

    async saveChecksumFile(
      algorithm: ChecksumAlgorithm,
      fileName: string,
      options?: { overwrite?: boolean },
    ): Promise<Location | undefined> {
      const { jobId } = context.getChecksumState();
      const directory = context.getActiveLocation();
      const trimmed = fileName.trim();
      if (jobId === undefined || directory === undefined || trimmed === '') return undefined;

      // Join onto the pane's directory URI rather than asking the host for a
      // native save dialog, so the file lands in the filesystem the user is
      // actually browsing and both hosts behave identically.
      const base = directory.uri.endsWith('/') ? directory.uri.slice(0, -1) : directory.uri;
      const destination: Location = {
        providerId: directory.providerId,
        uri: `${base}/${encodeURIComponent(trimmed)}`,
      };
      try {
        const saved = await context.getClient().saveChecksumFile(jobId, {
          destination,
          algorithm,
          ...(options?.overwrite === undefined ? {} : { overwrite: options.overwrite }),
        });
        context.setChecksumState(withChecksumSaved(context.getChecksumState(), saved.location));
        context.redraw();
        return saved.location;
      } catch (error: unknown) {
        context.setChecksumState(
          withChecksumError(
            context.getChecksumState(),
            message(error, t('checksums', 'unableToSaveFile')),
          ),
        );
        context.redraw();
        return undefined;
      }
    },

    verifyAgainst(content: string): void {
      const { jobId } = context.getChecksumState();
      if (jobId === undefined) return;
      void context
        .getClient()
        .verifyChecksumFile(jobId, content)
        .then((report) => {
          context.setChecksumState(withVerificationReport(context.getChecksumState(), report));
          context.redraw();
        })
        .catch((error: unknown) => {
          context.setChecksumState(
            withChecksumError(
              context.getChecksumState(),
              message(error, t('checksums', 'unableToVerify')),
            ),
          );
          context.redraw();
        });
    },

    handleChecksumBatch(
      jobId: string,
      entries: readonly ChecksumEntry[],
      isComplete: boolean,
      isCancelled: boolean,
    ): void {
      context.setChecksumState(
        withChecksumBatch(context.getChecksumState(), jobId, entries, isComplete, isCancelled),
      );
      context.redraw();
    },

    findDuplicates(): void {
      const workspace = context.getWorkspace();
      const root = context.getActiveLocation();
      if (workspace === undefined || root === undefined) return;

      const previousId = context.getDuplicateState().scanId;
      if (previousId !== undefined) {
        void context
          .getClient()
          .cancelDuplicateScan(previousId)
          .catch(() => undefined);
      }

      void context
        .getClient()
        .startDuplicateScan({ workspaceId: workspace.id, roots: [root] })
        .then((result) => {
          context.setDuplicateState(withDuplicateScanStarted(result.scanId, [root]));
          context.redraw();
        })
        .catch((error: unknown) => {
          context.setDuplicateState(
            withDuplicateError(
              context.getDuplicateState(),
              message(error, t('checksums', 'unableToStartDuplicateDetection')),
            ),
          );
          context.redraw();
        });
    },

    cancelDuplicateScan(): void {
      const { scanId } = context.getDuplicateState();
      if (scanId === undefined) return;
      void context
        .getClient()
        .cancelDuplicateScan(scanId)
        .catch(() => undefined);
      context.setDuplicateState(withDuplicateCleared());
      context.redraw();
    },

    closeDuplicates(): void {
      context.setDuplicateState(withDuplicateCleared());
      context.redraw();
    },

    toggleDuplicateSelection(uri: string): void {
      context.setDuplicateState(withDuplicateSelectionToggled(context.getDuplicateState(), uri));
      context.redraw();
    },

    deleteSelectedDuplicates(): void {
      const locations = selectedLocations(context.getDuplicateState());
      if (locations.length === 0) return;
      // Hand off to the shared delete flow; it owns the confirmation dialog.
      context.requestDelete(locations);
      context.setDuplicateState(withDuplicateSelectionCleared(context.getDuplicateState()));
      context.redraw();
    },

    handleDuplicateResults(
      scanId: string,
      groups: readonly DuplicateGroup[],
      isCancelled: boolean,
      warningsCount: number,
    ): void {
      context.setDuplicateState(
        withDuplicateResults(
          context.getDuplicateState(),
          scanId,
          groups,
          isCancelled,
          warningsCount,
        ),
      );
      context.redraw();
    },
  };
}
