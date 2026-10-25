import type { FileManagerClient } from '../../api/client/file-manager-client';
import { errorText, t } from '../../i18n';
import type { LinkProblem, LinkReplacementCandidate, Location, Operation } from '../../models';
import { parentLocation } from '../navigation/navigation';
import type { OperationsController } from '../operations/operations-controller';
import {
  INITIAL_LINK_DOCTOR_STATE,
  type LinkDoctorState,
  missingTargetName,
  problemKey,
  sourceName,
  withoutProblem,
} from './link-doctor-state';

const PAGE_SIZE = 500;
const MAX_PROBLEMS = 5000;
const POLL_INTERVAL_MS = 700;
const OPERATION_POLL_MS = 150;
const OPERATION_TIMEOUT_MS = 20_000;

export interface LinkDoctorControllerContext {
  getClient(): Pick<
    FileManagerClient,
    | 'startLinkDoctorScan'
    | 'getLinkDoctorScan'
    | 'cancelLinkDoctorScan'
    | 'findLinkReplacements'
    | 'listOperations'
  >;
  getOpsController(): Pick<OperationsController, 'trash' | 'createLink'>;
  getState(): LinkDoctorState;
  setState(state: LinkDoctorState): void;
  toast(message: string): void;
  redraw(): void;
  /** Opens a text file in the editor (the fix path for links inside documents). */
  openInEditor(location: Location): void;
  /** Re-lists panes after a link was repaired. */
  refreshPanes(): void;
}

export interface LinkDoctorController {
  start(roots: readonly Location[]): Promise<void>;
  cancel(): Promise<void>;
  close(): void;
  findReplacement(problem: LinkProblem): Promise<void>;
  repointSymlink(problem: LinkProblem, candidate: LinkReplacementCandidate): Promise<void>;
  openSource(problem: LinkProblem): void;
  dispose(): void;
}

function message(error: unknown): string {
  return errorText(error);
}

const TERMINAL_STATES: ReadonlySet<Operation['state']> = new Set([
  'completed',
  'completedWithWarnings',
  'failed',
  'cancelled',
  'interrupted',
]);

export function createLinkDoctorController(
  context: LinkDoctorControllerContext,
): LinkDoctorController {
  let timer: ReturnType<typeof setTimeout> | undefined;
  let generation = 0;
  /** Roots of the running scan; replacement searches cover the same tree. */
  let scanRoots: readonly Location[] = [];

  function patch(change: Partial<LinkDoctorState>): void {
    context.setState({ ...context.getState(), ...change });
    context.redraw();
  }

  function stopPolling(): void {
    generation += 1;
    if (timer !== undefined) clearTimeout(timer);
    timer = undefined;
  }

  async function poll(scanId: string, token: number): Promise<void> {
    try {
      const client = context.getClient();
      const page = await client.getLinkDoctorScan(scanId, { offset: 0, limit: PAGE_SIZE });
      if (token !== generation) return;
      const problems = [...page.problems];
      let hasMore = page.hasMore;
      while (page.isComplete && hasMore && problems.length < MAX_PROBLEMS) {
        const next = await client.getLinkDoctorScan(scanId, {
          offset: problems.length,
          limit: PAGE_SIZE,
        });
        problems.push(...next.problems);
        hasMore = next.hasMore && next.problems.length > 0;
      }
      if (token !== generation) return;
      patch({
        problems,
        entriesScanned: page.entriesScanned,
        linksChecked: page.linksChecked,
        isComplete: page.isComplete,
        isCancelled: page.isCancelled,
      });
      if (!page.isComplete) {
        timer = setTimeout(() => void poll(scanId, token), POLL_INTERVAL_MS);
      }
    } catch (error: unknown) {
      if (token === generation) patch({ error: message(error), isComplete: true });
    }
  }

  async function waitForOperation(operation: Operation): Promise<Operation['state']> {
    const deadline = Date.now() + OPERATION_TIMEOUT_MS;
    let state = operation.state;
    while (!TERMINAL_STATES.has(state) && Date.now() < deadline) {
      await new Promise((resolve) => setTimeout(resolve, OPERATION_POLL_MS));
      const current = (await context.getClient().listOperations()).find(
        (candidate) => candidate.id === operation.id,
      );
      if (current !== undefined) state = current.state;
    }
    return state;
  }

  return {
    async start(roots) {
      stopPolling();
      const token = generation;
      scanRoots = roots;
      context.setState({ ...INITIAL_LINK_DOCTOR_STATE, scanId: 'starting' });
      context.redraw();
      try {
        const started = await context.getClient().startLinkDoctorScan({ roots });
        if (token !== generation) return;
        patch({ scanId: started.scanId });
        await poll(started.scanId, token);
      } catch (error: unknown) {
        if (token !== generation) return;
        context.setState(INITIAL_LINK_DOCTOR_STATE);
        context.toast(t('linkDoctor', 'startFailed', { message: message(error) }));
        context.redraw();
      }
    },

    async cancel() {
      const { scanId } = context.getState();
      if (scanId === undefined || scanId === 'starting') return;
      try {
        await context.getClient().cancelLinkDoctorScan(scanId);
      } catch (error: unknown) {
        patch({ error: message(error) });
      }
    },

    close() {
      stopPolling();
      const { scanId, isComplete } = context.getState();
      if (scanId !== undefined && scanId !== 'starting' && !isComplete) {
        void context
          .getClient()
          .cancelLinkDoctorScan(scanId)
          .catch(() => undefined);
      }
      context.setState(INITIAL_LINK_DOCTOR_STATE);
      context.redraw();
    },

    async findReplacement(problem) {
      const key = problemKey(problem);
      const state = context.getState();
      patch({
        candidates: {
          ...state.candidates,
          [key]: { loading: true, candidates: [], truncated: false },
        },
      });
      try {
        const result = await context.getClient().findLinkReplacements({
          roots: scanRoots,
          targetName: missingTargetName(problem),
        });
        patch({
          candidates: {
            ...context.getState().candidates,
            [key]: { loading: false, candidates: result.candidates, truncated: result.truncated },
          },
        });
      } catch (error: unknown) {
        patch({
          error: message(error),
          candidates: {
            ...context.getState().candidates,
            [key]: { loading: false, candidates: [], truncated: false },
          },
        });
      }
    },

    async repointSymlink(problem, candidate) {
      if (problem.kind !== 'brokenSymlink') return;
      const key = problemKey(problem);
      patch({ repairing: key, error: undefined });
      try {
        const ops = context.getOpsController();
        const trashed = await ops.trash([problem.source]);
        if (trashed === undefined) {
          patch({ repairing: undefined });
          return;
        }
        const trashState = await waitForOperation(trashed);
        if (trashState !== 'completed' && trashState !== 'completedWithWarnings') {
          throw new Error(`the old link could not be moved to the Trash (${trashState})`);
        }
        await ops.createLink(
          candidate.location,
          parentLocation(problem.source),
          sourceName(problem),
          { kind: 'symbolicLink', targetStyle: 'relative' },
        );
        context.setState(withoutProblem({ ...context.getState(), repairing: undefined }, key));
        context.toast(t('linkDoctor', 'fixed', { name: sourceName(problem) }));
        context.refreshPanes();
        context.redraw();
      } catch (error: unknown) {
        patch({ repairing: undefined });
        context.toast(t('linkDoctor', 'fixFailed', { message: message(error) }));
      }
    },

    openSource(problem) {
      context.openInEditor(problem.source);
    },

    dispose() {
      stopPolling();
    },
  };
}
