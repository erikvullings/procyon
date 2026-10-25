import type { FileManagerClient } from '../../api/client/file-manager-client';
import { errorText, t } from '../../i18n';
import type { SnapshotDiffEntry, StartSnapshotJobRequest } from '../../models';
import { INITIAL_SNAPSHOT_PANEL_STATE, type SnapshotPanelState } from './snapshot-state';

const PAGE_SIZE = 500;
const MAX_ENTRIES = 20_000;
const POLL_INTERVAL_MS = 700;

export interface SnapshotControllerContext {
  getClient(): Pick<FileManagerClient, 'startSnapshotJob' | 'getSnapshotJob' | 'cancelSnapshotJob'>;
  getState(): SnapshotPanelState;
  setState(state: SnapshotPanelState): void;
  toast(message: string): void;
  redraw(): void;
  /** Re-lists panes after a snapshot file was written. */
  refreshPanes(): void;
}

export interface SnapshotController {
  start(request: StartSnapshotJobRequest): Promise<void>;
  cancel(): Promise<void>;
  close(): void;
  dispose(): void;
}

function message(error: unknown): string {
  return errorText(error);
}

export function createSnapshotController(context: SnapshotControllerContext): SnapshotController {
  let timer: ReturnType<typeof setTimeout> | undefined;
  let generation = 0;

  function patch(change: Partial<SnapshotPanelState>): void {
    context.setState({ ...context.getState(), ...change });
    context.redraw();
  }

  function stopPolling(): void {
    generation += 1;
    if (timer !== undefined) clearTimeout(timer);
    timer = undefined;
  }

  async function poll(jobId: string, token: number): Promise<void> {
    try {
      const client = context.getClient();
      const page = await client.getSnapshotJob(jobId, { offset: 0, limit: PAGE_SIZE });
      if (token !== generation) return;
      const entries: SnapshotDiffEntry[] = [...page.entries];
      let hasMore = page.hasMore;
      while (page.isComplete && hasMore && entries.length < MAX_ENTRIES) {
        const next = await client.getSnapshotJob(jobId, {
          offset: entries.length,
          limit: PAGE_SIZE,
        });
        entries.push(...next.entries);
        hasMore = next.hasMore && next.entries.length > 0;
      }
      if (token !== generation) return;
      patch({ page, entries: page.isComplete ? entries : [] });
      if (page.isComplete) {
        if (page.kind === 'create' && page.error == null) context.refreshPanes();
      } else {
        timer = setTimeout(() => void poll(jobId, token), POLL_INTERVAL_MS);
      }
    } catch (error: unknown) {
      if (token === generation) patch({ error: message(error) });
    }
  }

  return {
    async start(request) {
      stopPolling();
      const token = generation;
      context.setState({ ...INITIAL_SNAPSHOT_PANEL_STATE, jobId: 'starting', request });
      context.redraw();
      try {
        const started = await context.getClient().startSnapshotJob(request);
        if (token !== generation) return;
        patch({ jobId: started.jobId });
        await poll(started.jobId, token);
      } catch (error: unknown) {
        if (token !== generation) return;
        context.setState(INITIAL_SNAPSHOT_PANEL_STATE);
        context.toast(t('snapshots', 'startFailed', { message: message(error) }));
        context.redraw();
      }
    },

    async cancel() {
      const { jobId } = context.getState();
      if (jobId === undefined || jobId === 'starting') return;
      try {
        await context.getClient().cancelSnapshotJob(jobId);
      } catch (error: unknown) {
        patch({ error: message(error) });
      }
    },

    close() {
      stopPolling();
      const { jobId, page } = context.getState();
      if (jobId !== undefined && jobId !== 'starting' && page?.isComplete !== true) {
        void context
          .getClient()
          .cancelSnapshotJob(jobId)
          .catch(() => undefined);
      }
      context.setState(INITIAL_SNAPSHOT_PANEL_STATE);
      context.redraw();
    },

    dispose() {
      stopPolling();
    },
  };
}
