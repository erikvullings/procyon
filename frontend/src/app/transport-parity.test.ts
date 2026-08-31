import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { MockFileManagerClient } from '../api/client/mock-file-manager-client';
import type { EventStream } from '../api/events/event-stream';
import { SseEventStream } from '../api/events/sse-event-stream';
import { type TauriChannelLike, TauriEventStream } from '../api/events/tauri-event-stream';
import type { BackendEvent, Unsubscribe } from '../models';
import type { RuntimeKind } from '../utilities/runtime';
import { AppShell } from './app-shell';

class ScenarioClient extends MockFileManagerClient {
  constructor(private readonly stream: EventStream) {
    super();
  }

  override async subscribe(listener: (event: BackendEvent) => void): Promise<Unsubscribe> {
    const unsubscribe = this.stream.listeners.subscribe(listener);
    await this.stream.connect();
    return unsubscribe;
  }

  override onResynchronise(listener: () => void): Unsubscribe {
    return this.stream.resynchronise.subscribe(listener);
  }

  override disconnect(): void {
    this.stream.close();
  }
}

class ScenarioEventSource extends EventTarget {
  static current: ScenarioEventSource | undefined;

  constructor(_url: string | URL) {
    super();
    ScenarioEventSource.current = this;
  }

  close(): void {}

  emit(type: string, json: string): void {
    this.dispatchEvent(new MessageEvent(type, { data: json }));
  }
}

interface ScenarioTransport {
  runtime: RuntimeKind;
  client: ScenarioClient;
  emit(event: BackendEvent): void;
}

function createHttpScenario(): ScenarioTransport {
  const stream = new SseEventStream({
    eventSource: ScenarioEventSource,
    requestFrame: (callback) => {
      callback(0);
      return 0;
    },
  });
  return {
    runtime: 'http',
    client: new ScenarioClient(stream),
    emit: (event) => ScenarioEventSource.current?.emit(event.payload.type, JSON.stringify(event)),
  };
}

function createTauriScenario(): ScenarioTransport {
  let channel: TauriChannelLike<string> | undefined;
  const stream = new TauriEventStream({
    invoke: async <T>() => 'mock-subscription' as T,
    createChannel: (onmessage) => {
      channel = { onmessage };
      return channel;
    },
    requestFrame: (callback) => {
      callback(0);
      return 0;
    },
  });
  return {
    runtime: 'tauri',
    client: new ScenarioClient(stream),
    emit: (event) => channel?.onmessage(JSON.stringify(event)),
  };
}

let root: HTMLElement;

beforeEach(() => {
  root = document.createElement('div');
  document.body.appendChild(root);
});

afterEach(() => {
  m.mount(root, null);
  root.remove();
  ScenarioEventSource.current = undefined;
});

describe.each([
  ['HTTP/SSE', createHttpScenario],
  ['Tauri channel', createTauriScenario],
] as const)('%s frontend parity scenario', (_name, createScenario) => {
  it('navigates, receives an external file delta, and applies it', async () => {
    const scenario = createScenario();
    m.mount(root, {
      view: () => m(AppShell, { runtime: scenario.runtime, client: scenario.client }),
    });
    await expect.poll(() => root.textContent).toContain('Documents');

    scenario.emit({
      eventId: 34,
      timestamp: '2026-07-31T12:00:00Z',
      payload: {
        type: 'directory.delta',
        paneId: 'left',
        delta: {
          type: 'entriesAdded',
          revision: 2,
          entries: [
            {
              id: 'mock:///external.txt',
              location: { providerId: 'file', uri: 'mock:///external.txt' },
              name: 'external.txt',
              kind: 'file',
              size: 7,
              hidden: false,
              readOnly: false,
              extension: 'txt',
              metadataRevision: 1,
            },
          ],
        },
      },
    });

    await expect
      .poll(() =>
        [...root.querySelectorAll('.fm-entry-name [title]')].some(
          (name) => name.getAttribute('title') === 'external.txt',
        ),
      )
      .toBe(true);
  });
});
