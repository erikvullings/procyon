/**
 * Tells every other window that a failed semantic job was acknowledged, so its warning leaves
 * all windows' activity lists, not only the one where it was opened. Windows of either host
 * share an origin, so a `BroadcastChannel` reaches them without backend involvement.
 */
const CHANNEL_NAME = 'fm.acknowledgedSemanticJobs';

export interface AcknowledgedJobsChannel {
  /** Announces `jobId` as acknowledged to the other windows. */
  acknowledge(jobId: string): void;
  close(): void;
}

/** Opens the channel; `onAcknowledged` runs for jobs acknowledged in *other* windows. */
export function openAcknowledgedJobsChannel(
  onAcknowledged: (jobId: string) => void,
): AcknowledgedJobsChannel {
  if (typeof BroadcastChannel === 'undefined') {
    return { acknowledge: () => undefined, close: () => undefined };
  }
  const channel = new BroadcastChannel(CHANNEL_NAME);
  channel.onmessage = (event: MessageEvent<unknown>) => {
    if (typeof event.data === 'string') onAcknowledged(event.data);
  };
  return {
    acknowledge: (jobId) => channel.postMessage(jobId),
    close: () => channel.close(),
  };
}
