import { Channel, invoke } from '@tauri-apps/api/core';

import type { BackendEvent } from '../../models';
import {
  BackendEventListenerRegistry,
  type EventStream,
  EventStreamSignalRegistry,
  MutableEventStreamStatus,
  parseBackendEvent,
} from './event-stream';

const HIGH_FREQUENCY_TYPES = new Set<string>([
  'operation.progress',
  'directory.delta',
  'search.resultsBatch',
  'comparison.resultsBatch',
]);

export interface TauriChannelLike<T> {
  onmessage: (message: T) => void;
}

export interface TauriEventStreamDependencies {
  invoke: <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
  createChannel: (onmessage: (message: string) => void) => TauriChannelLike<string>;
  requestFrame?: (callback: FrameRequestCallback) => number;
}

const defaultDependencies: TauriEventStreamDependencies = {
  invoke,
  createChannel: (onmessage) => new Channel<string>(onmessage),
};

/**
 * Ordered desktop event stream backed by one Tauri IPC channel.
 *
 * Unlike SSE, a Tauri channel has no observable network-disconnected state:
 * status is `connecting` while the Rust subscription is installed, `open`
 * until explicit application shutdown, and `closed` after shutdown or setup
 * failure. It therefore never reports `reconnecting`.
 */
export class TauriEventStream implements EventStream {
  readonly status = new MutableEventStreamStatus();
  readonly listeners = new BackendEventListenerRegistry();
  readonly resynchronise = new EventStreamSignalRegistry();

  private readonly dependencies: Required<TauriEventStreamDependencies>;
  private subscriptionId: string | undefined;
  private connecting: Promise<void> | undefined;
  private channel: TauriChannelLike<string> | undefined;
  private queued: BackendEvent[] = [];
  private framePending = false;
  private desiredOpen = false;

  constructor(dependencies: TauriEventStreamDependencies = defaultDependencies) {
    this.dependencies = {
      ...dependencies,
      requestFrame:
        dependencies.requestFrame ?? ((callback) => globalThis.requestAnimationFrame(callback)),
    };
  }

  connect(): Promise<void> {
    if (this.subscriptionId !== undefined) return Promise.resolve();
    this.desiredOpen = true;
    if (this.connecting !== undefined) return this.connecting;
    this.status.set('connecting');
    const channel = this.dependencies.createChannel(this.handleMessage);
    this.channel = channel;
    this.connecting = this.dependencies
      .invoke<string>('subscribe_events', { onEvent: channel })
      .then((subscriptionId) => {
        if (!this.desiredOpen) {
          this.channel = undefined;
          void this.release(subscriptionId);
          return;
        }
        this.subscriptionId = subscriptionId;
        this.status.set('open');
      })
      .catch((error: unknown) => {
        this.channel = undefined;
        this.status.set('closed');
        throw error;
      })
      .finally(() => {
        this.connecting = undefined;
      });
    return this.connecting;
  }

  close(): void {
    this.desiredOpen = false;
    const subscriptionId = this.subscriptionId;
    this.subscriptionId = undefined;
    this.channel = undefined;
    this.queued = [];
    this.framePending = false;
    this.status.set('closed');
    if (subscriptionId !== undefined) {
      void this.release(subscriptionId);
    }
  }

  private async release(subscriptionId: string): Promise<void> {
    try {
      await this.dependencies.invoke<void>('unsubscribe_events', { subscriptionId });
    } catch {
      // Rust also releases subscriptions on window destruction. Teardown is
      // best-effort because the webview may already be closing.
    }
  }

  private readonly handleMessage = (message: string): void => {
    if (this.channel === undefined || this.status.get() !== 'open') return;
    let decoded: unknown;
    try {
      decoded = JSON.parse(message);
    } catch {
      return;
    }
    if (
      typeof decoded === 'object' &&
      decoded !== null &&
      'type' in decoded &&
      decoded.type === 'resynchronise'
    ) {
      this.resynchronise.dispatch();
      return;
    }
    const event = parseBackendEvent(decoded);
    if (event === undefined) return;
    if (HIGH_FREQUENCY_TYPES.has(event.payload.type)) {
      this.queued.push(event);
      if (!this.framePending) {
        this.framePending = true;
        this.dependencies.requestFrame(() => this.flush());
      }
    } else {
      this.listeners.dispatch(event);
    }
  };

  private flush(): void {
    this.framePending = false;
    const events = this.queued;
    this.queued = [];
    for (const event of events) this.listeners.dispatch(event);
  }
}
