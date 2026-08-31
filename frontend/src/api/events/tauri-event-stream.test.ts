import { describe, expect, it, vi } from 'vitest';

import type { BackendEvent } from '../../models';
import {
  type TauriChannelLike,
  TauriEventStream,
  type TauriEventStreamDependencies,
} from './tauri-event-stream';

interface TestChannel extends TauriChannelLike<string> {
  emit(message: string): void;
}

function harness() {
  let channel: TestChannel | undefined;
  const invoke = vi.fn().mockResolvedValue('subscription-1');
  const dependencies: TauriEventStreamDependencies = {
    invoke,
    createChannel: (onmessage) => {
      channel = {
        onmessage,
        emit(message) {
          this.onmessage(message);
        },
      };
      return channel;
    },
  };
  return { dependencies, invoke, channel: () => channel };
}

function envelope(eventId: number, type = 'runtime.ready'): string {
  return JSON.stringify({ eventId, timestamp: '2026-07-31T12:00:00Z', payload: { type } });
}

describe('TauriEventStream', () => {
  it('transitions connecting → open after the host accepts one channel', async () => {
    const { dependencies, invoke, channel } = harness();
    const stream = new TauriEventStream(dependencies);
    const statuses: string[] = [];
    stream.status.subscribe((status) => statuses.push(status));

    await Promise.all([stream.connect(), stream.connect()]);

    expect(statuses).toEqual(['connecting', 'open']);
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith('subscribe_events', { onEvent: channel() });
  });

  it('parses channel JSON and batches the same high-frequency types as SSE', async () => {
    const frames: FrameRequestCallback[] = [];
    const { dependencies, channel } = harness();
    const stream = new TauriEventStream({
      ...dependencies,
      requestFrame: (callback) => frames.push(callback) - 1,
    });
    const received: BackendEvent[] = [];
    stream.listeners.subscribe((event) => received.push(event));
    await stream.connect();

    channel()?.emit(envelope(1, 'operation.progress'));
    channel()?.emit(envelope(2, 'directory.delta'));
    channel()?.emit(envelope(3));

    expect(received.map(({ eventId }) => eventId)).toEqual([3]);
    expect(frames).toHaveLength(1);
    frames[0]?.(0);
    expect(received.map(({ eventId }) => eventId)).toEqual([3, 1, 2]);
  });

  it('ignores malformed and unknown messages without closing the channel', async () => {
    const { dependencies, channel } = harness();
    const stream = new TauriEventStream(dependencies);
    const listener = vi.fn();
    stream.listeners.subscribe(listener);
    await stream.connect();

    channel()?.emit('{');
    channel()?.emit(envelope(1, 'future.event'));
    channel()?.emit(envelope(2));

    expect(listener).toHaveBeenCalledTimes(1);
  });

  it('reports replay gaps as resynchronisation signals', async () => {
    const { dependencies, channel } = harness();
    const stream = new TauriEventStream(dependencies);
    const resynchronise = vi.fn();
    stream.resynchronise.subscribe(resynchronise);
    await stream.connect();

    channel()?.emit(
      JSON.stringify({
        type: 'resynchronise',
        lastEventId: 1,
        oldestAvailableId: 4,
        newestAvailableId: 8,
      }),
    );

    expect(resynchronise).toHaveBeenCalledOnce();
  });

  it('releases the Rust subscription on close and remains closed', async () => {
    const { dependencies, invoke, channel } = harness();
    const stream = new TauriEventStream(dependencies);
    const listener = vi.fn();
    stream.listeners.subscribe(listener);
    await stream.connect();

    stream.close();
    channel()?.emit(envelope(1));

    expect(stream.status.get()).toBe('closed');
    expect(listener).not.toHaveBeenCalled();
    expect(invoke).toHaveBeenLastCalledWith('unsubscribe_events', {
      subscriptionId: 'subscription-1',
    });
  });

  it('returns to closed when channel setup fails', async () => {
    const failure = new Error('channel rejected');
    const { dependencies } = harness();
    dependencies.invoke = vi.fn().mockRejectedValue(failure);
    const stream = new TauriEventStream(dependencies);

    await expect(stream.connect()).rejects.toBe(failure);

    expect(stream.status.get()).toBe('closed');
  });

  it('releases a subscription that finishes connecting after shutdown', async () => {
    let resolveSubscription: ((id: string) => void) | undefined;
    const { dependencies, invoke } = harness();
    invoke.mockReturnValue(
      new Promise<string>((resolve) => {
        resolveSubscription = resolve;
      }),
    );
    const stream = new TauriEventStream(dependencies);

    const connecting = stream.connect();
    stream.close();
    resolveSubscription?.('late-subscription');
    await connecting;

    expect(stream.status.get()).toBe('closed');
    expect(invoke).toHaveBeenLastCalledWith('unsubscribe_events', {
      subscriptionId: 'late-subscription',
    });
  });
});
