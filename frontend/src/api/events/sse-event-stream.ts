import type { BackendEvent } from '../../models';
import {
  BackendEventListenerRegistry,
  type EventStream,
  EventStreamSignalRegistry,
  MutableEventStreamStatus,
  parseBackendEvent,
} from './event-stream';

const EVENT_TYPES = [
  'runtime.ready',
  'workspace.created',
  'workspace.renamed',
  'workspace.opened',
  'workspace.closed',
  'workspace.deleted',
  'workspace.layoutChanged',
  'workspace.activePaneChanged',
  'workspace.tabAdded',
  'workspace.tabClosed',
  'workspace.tabActivated',
  'workspace.tabNavigated',
  'workspace.tabViewChanged',
  'directory.snapshot',
  'directory.delta',
  'operation.created',
  'operation.progress',
  'operation.stateChanged',
  'operation.conflict',
  'operation.completed',
  'operation.failed',
  'plugin.changed',
  'notification.created',
  'search.resultsBatch',
  'comparison.resultsBatch',
  'diskUsage.progress',
  'diskUsage.finalizing',
  'diskUsage.failed',
] as const;

const HIGH_FREQUENCY_TYPES = new Set<string>([
  'operation.progress',
  'directory.delta',
  'search.resultsBatch',
  'comparison.resultsBatch',
  'diskUsage.progress',
]);

interface EventSourceLike extends EventTarget {
  close(): void;
}

function BrowserEventSource(
  this: EventSourceLike,
  url: string | URL,
  init?: EventSourceInit,
): EventSourceLike {
  return new EventSource(url, init);
}

export interface EventSourceConstructor {
  new (url: string | URL, eventSourceInitDict?: EventSourceInit): EventSourceLike;
}

export interface SseEventStreamOptions {
  readonly url?: string;
  readonly eventSource?: EventSourceConstructor;
  readonly random?: () => number;
  readonly reconnectBaseMs?: number;
  readonly reconnectMaxMs?: number;
  readonly staleTimeoutMs?: number;
  readonly requestFrame?: (callback: FrameRequestCallback) => number;
  /**
   * Supplies the current session token, appended as `?token=` on every
   * (re)connect. Browser `EventSource` connections can't set custom headers,
   * so the token travels in the query string instead of `Authorization`
   * (task 0064 backend, frontend follow-up). Read fresh on every connect
   * attempt so a token entered after construction, or rotated between
   * reconnects, is picked up.
   */
  readonly tokenProvider?: () => string | undefined;
}

/** Returns exponential backoff with ±50% jitter, capped before jitter is applied. */
export function reconnectDelay(
  attempt: number,
  baseMs: number,
  maxMs: number,
  random: number,
): number {
  const exponential = baseMs * 2 ** attempt;
  return Math.min(maxMs, Math.round(exponential * (0.5 + random)));
}

/** One browser SSE connection with explicit reconnect and liveness management. */
export class SseEventStream implements EventStream {
  readonly status = new MutableEventStreamStatus();
  readonly listeners = new BackendEventListenerRegistry();
  readonly resynchronise = new EventStreamSignalRegistry();

  private readonly options: Required<
    Pick<
      SseEventStreamOptions,
      | 'url'
      | 'eventSource'
      | 'random'
      | 'reconnectBaseMs'
      | 'reconnectMaxMs'
      | 'staleTimeoutMs'
      | 'requestFrame'
    >
  >;
  private readonly tokenProvider: (() => string | undefined) | undefined;
  private source: EventSourceLike | undefined;
  private reconnectTimer: ReturnType<typeof setTimeout> | undefined;
  private staleTimer: ReturnType<typeof setTimeout> | undefined;
  private attempt = 0;
  private lastEventId: number | undefined;
  private closed = true;
  private queued: BackendEvent[] = [];
  private framePending = false;

  constructor(options: SseEventStreamOptions = {}) {
    this.options = {
      url: options.url ?? '/api/v1/events',
      eventSource: options.eventSource ?? (BrowserEventSource as unknown as EventSourceConstructor),
      random: options.random ?? Math.random,
      reconnectBaseMs: options.reconnectBaseMs ?? 1_000,
      reconnectMaxMs: options.reconnectMaxMs ?? 30_000,
      staleTimeoutMs: options.staleTimeoutMs ?? 45_000,
      requestFrame:
        options.requestFrame ?? ((callback) => globalThis.requestAnimationFrame(callback)),
    };
    this.tokenProvider = options.tokenProvider;
  }

  async connect(): Promise<void> {
    if (this.source !== undefined || !this.closed) return;
    this.closed = false;
    this.status.set('connecting');
    this.openSource();
  }

  close(): void {
    this.closed = true;
    this.clearTimers();
    this.detachSource();
    this.source = undefined;
    this.queued = [];
    this.framePending = false;
    this.status.set('closed');
  }

  private openSource(): void {
    const url = new URL(this.options.url, globalThis.location?.href ?? 'http://localhost');
    if (this.lastEventId !== undefined)
      url.searchParams.set('lastEventId', String(this.lastEventId));
    const token = this.tokenProvider?.();
    if (token !== undefined && token.length > 0) url.searchParams.set('token', token);
    const source = new this.options.eventSource(url);
    this.source = source;
    source.addEventListener('open', this.handleOpen);
    source.addEventListener('error', this.handleError);
    source.addEventListener('keep-alive', this.handleActivity);
    source.addEventListener('resynchronise', this.handleGap);
    for (const type of EVENT_TYPES) source.addEventListener(type, this.handleEvent);
  }

  private readonly handleOpen = (): void => {
    this.attempt = 0;
    this.status.set('open');
    this.armStaleTimer();
  };

  private readonly handleActivity = (): void => this.armStaleTimer();

  private readonly handleGap = (): void => {
    this.armStaleTimer();
    this.resynchronise.dispatch();
  };

  private readonly handleEvent = (raw: Event): void => {
    this.armStaleTimer();
    if (!(raw instanceof MessageEvent) || typeof raw.data !== 'string') return;
    let decoded: unknown;
    try {
      decoded = JSON.parse(raw.data);
    } catch {
      return;
    }
    const event = parseBackendEvent(decoded);
    if (event === undefined) return;
    this.lastEventId = event.eventId;
    if (HIGH_FREQUENCY_TYPES.has(event.payload.type)) {
      this.queued.push(event);
      if (!this.framePending) {
        this.framePending = true;
        this.options.requestFrame(() => this.flush());
      }
    } else {
      this.listeners.dispatch(event);
    }
  };

  private readonly handleError = (): void => this.scheduleReconnect();

  private flush(): void {
    this.framePending = false;
    const events = this.queued;
    this.queued = [];
    for (const event of events) this.listeners.dispatch(event);
  }

  private armStaleTimer(): void {
    if (this.staleTimer !== undefined) clearTimeout(this.staleTimer);
    this.staleTimer = setTimeout(() => this.scheduleReconnect(), this.options.staleTimeoutMs);
  }

  private scheduleReconnect(): void {
    if (this.closed || this.reconnectTimer !== undefined) return;
    if (this.staleTimer !== undefined) clearTimeout(this.staleTimer);
    this.detachSource();
    this.status.set('reconnecting');
    const delay = reconnectDelay(
      this.attempt,
      this.options.reconnectBaseMs,
      this.options.reconnectMaxMs,
      this.options.random(),
    );
    this.attempt += 1;
    this.reconnectTimer = setTimeout(() => {
      this.reconnectTimer = undefined;
      if (!this.closed) this.openSource();
    }, delay);
  }

  private clearTimers(): void {
    if (this.reconnectTimer !== undefined) clearTimeout(this.reconnectTimer);
    if (this.staleTimer !== undefined) clearTimeout(this.staleTimer);
    this.reconnectTimer = undefined;
    this.staleTimer = undefined;
  }

  private detachSource(): void {
    const source = this.source;
    if (source === undefined) return;
    source.removeEventListener('open', this.handleOpen);
    source.removeEventListener('error', this.handleError);
    source.removeEventListener('keep-alive', this.handleActivity);
    source.removeEventListener('resynchronise', this.handleGap);
    for (const type of EVENT_TYPES) source.removeEventListener(type, this.handleEvent);
    source.close();
    this.source = undefined;
  }
}
