import { afterEach, describe, expect, it, vi } from 'vitest';

import type { BackendEvent } from '../../models';
import { reconnectDelay, SseEventStream } from './sse-event-stream';

class FakeEventSource extends EventTarget {
  static instances: FakeEventSource[] = [];
  readonly url: string;
  closed = false;

  constructor(url: string | URL) {
    super();
    this.url = String(url);
    FakeEventSource.instances.push(this);
  }

  close(): void {
    this.closed = true;
  }

  emit(type: string, data = '', lastEventId = ''): void {
    this.dispatchEvent(new MessageEvent(type, { data, lastEventId }));
  }
}

function envelope(eventId: number, type = 'runtime.ready'): string {
  return JSON.stringify({ eventId, timestamp: '2026-07-31T12:00:00Z', payload: { type } });
}

afterEach(() => {
  vi.useRealTimers();
  FakeEventSource.instances = [];
});

describe('reconnectDelay', () => {
  it('uses capped exponential backoff with symmetric jitter', () => {
    expect(reconnectDelay(0, 1_000, 8_000, 0)).toBe(500);
    expect(reconnectDelay(1, 1_000, 8_000, 0.5)).toBe(2_000);
    expect(reconnectDelay(9, 1_000, 8_000, 1)).toBe(8_000);
  });
});

describe('SseEventStream', () => {
  it('maintains one connection, reconnects after errors, and resumes from the last event id', async () => {
    vi.useFakeTimers();
    const stream = new SseEventStream({
      eventSource: FakeEventSource,
      random: () => 0.5,
      reconnectBaseMs: 1_000,
    });
    await stream.connect();
    await stream.connect();
    const first = FakeEventSource.instances[0];
    expect(FakeEventSource.instances).toHaveLength(1);
    first?.dispatchEvent(new Event('open'));
    first?.emit('runtime.ready', envelope(42), '42');
    first?.dispatchEvent(new Event('error'));

    expect(FakeEventSource.instances).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(1_000);

    expect(FakeEventSource.instances).toHaveLength(2);
    expect(FakeEventSource.instances[1]?.url).toContain('lastEventId=42');
    expect(first?.closed).toBe(true);
  });

  it('appends the current token as a query parameter on every connect', async () => {
    let token: string | undefined = 'first-token';
    const stream = new SseEventStream({
      eventSource: FakeEventSource,
      tokenProvider: () => token,
    });
    await stream.connect();
    expect(FakeEventSource.instances[0]?.url).toContain('token=first-token');

    stream.close();
    token = 'second-token';
    await stream.connect();
    expect(FakeEventSource.instances[1]?.url).toContain('token=second-token');
  });

  it('omits the token query parameter when no token is available', async () => {
    const stream = new SseEventStream({
      eventSource: FakeEventSource,
      tokenProvider: () => undefined,
    });
    await stream.connect();
    expect(FakeEventSource.instances[0]?.url).not.toContain('token=');
  });

  it('forces a reconnect when no observable event or keep-alive arrives', async () => {
    vi.useFakeTimers();
    const stream = new SseEventStream({
      eventSource: FakeEventSource,
      random: () => 0.5,
      staleTimeoutMs: 20_000,
      reconnectBaseMs: 1_000,
    });
    await stream.connect();
    FakeEventSource.instances[0]?.dispatchEvent(new Event('open'));

    await vi.advanceTimersByTimeAsync(21_000);

    expect(FakeEventSource.instances).toHaveLength(2);
    expect(FakeEventSource.instances[0]?.closed).toBe(true);
  });

  it('keeps a healthy idle connection open when observable keep-alives arrive', async () => {
    vi.useFakeTimers();
    const stream = new SseEventStream({
      eventSource: FakeEventSource,
      staleTimeoutMs: 20_000,
    });
    await stream.connect();
    const source = FakeEventSource.instances[0];
    source?.dispatchEvent(new Event('open'));
    await vi.advanceTimersByTimeAsync(15_000);
    source?.emit('keep-alive');
    await vi.advanceTimersByTimeAsync(15_000);

    expect(FakeEventSource.instances).toHaveLength(1);
  });

  it('batches high-frequency events into one scheduled dispatch', async () => {
    const frames: FrameRequestCallback[] = [];
    const stream = new SseEventStream({
      eventSource: FakeEventSource,
      requestFrame: (callback) => frames.push(callback) - 1,
    });
    const received: BackendEvent[] = [];
    stream.listeners.subscribe((event) => received.push(event));
    await stream.connect();
    const source = FakeEventSource.instances[0];
    source?.emit('operation.progress', envelope(1, 'operation.progress'));
    source?.emit('directory.delta', envelope(2, 'directory.delta'));

    expect(received).toEqual([]);
    expect(frames).toHaveLength(1);
    frames[0]?.(0);
    expect(received.map((event) => event.eventId)).toEqual([1, 2]);
  });

  it('subscribes to and dispatches disk-usage progress events', async () => {
    const frames: FrameRequestCallback[] = [];
    const stream = new SseEventStream({
      eventSource: FakeEventSource,
      requestFrame: (callback) => frames.push(callback) - 1,
    });
    const received: BackendEvent[] = [];
    stream.listeners.subscribe((event) => received.push(event));
    await stream.connect();

    FakeEventSource.instances[0]?.emit('diskUsage.progress', envelope(3, 'diskUsage.progress'));
    frames[0]?.(0);

    expect(received.map((event) => event.payload.type)).toEqual(['diskUsage.progress']);
  });

  it('reports gaps separately and closes without retaining source listeners', async () => {
    const gap = vi.fn();
    const stream = new SseEventStream({ eventSource: FakeEventSource });
    stream.resynchronise.subscribe(gap);
    await stream.connect();
    const source = FakeEventSource.instances[0];
    source?.emit('resynchronise', '{}');
    stream.close();
    source?.emit('resynchronise', '{}');

    expect(gap).toHaveBeenCalledTimes(1);
    expect(stream.status.get()).toBe('closed');
  });
});
