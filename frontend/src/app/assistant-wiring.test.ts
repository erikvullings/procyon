import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { FileManagerClient } from '../api/client/file-manager-client';
import type { AssistantTabEntry } from '../features/assistant/assistant-tab-state';
import type { FileAssistantEvent, FileAssistantRequest, LlmProfile } from '../models';
import { createAssistantWiring } from './assistant-wiring';
import type { ShellState } from './shell-state';

const profile = { id: 'p1', name: 'Local' } as LlmProfile;

function fakeShell(
  client: Partial<FileManagerClient>,
  sourceControllerSetContent = vi.fn(),
): { shell: ShellState; entry: AssistantTabEntry; setContent: ReturnType<typeof vi.fn> } {
  const entry: AssistantTabEntry = {
    key: 'right:t1',
    paneId: 'right',
    tabId: 't1',
    sourcePaneId: 'left',
    fileName: 'script.sh',
    profiles: [profile],
    profileId: 'p1',
    messages: [],
  };
  const shell = {
    client: client as FileManagerClient,
    assistantByTab: new Map([[entry.key, entry]]),
    assistantOpening: false,
    editorByPane: new Map([
      [
        'left',
        {
          controller: { setContent: sourceControllerSetContent },
          state: { status: 'loaded', entry: { name: 'script.sh' } },
        },
      ],
    ]),
  } as unknown as ShellState;
  return { shell, entry, setContent: sourceControllerSetContent };
}

beforeEach(() => {
  vi.stubGlobal('crypto', { randomUUID: () => 'req-1' });
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('file assistant wiring', () => {
  it('streams fragments into the chat, then appends the final reply and clears the request', async () => {
    const seen: FileAssistantRequest[] = [];
    const client = {
      streamFileAssistantReply: vi.fn(
        (request: FileAssistantRequest, onEvent: (event: FileAssistantEvent) => void) => {
          seen.push(request);
          onEvent({ type: 'delta', text: '#!/usr' });
          onEvent({ type: 'delta', text: '/bin/bash\n' });
          onEvent({ type: 'done', text: '#!/usr/bin/env bash\n' });
          return Promise.resolve();
        },
      ),
    };
    const { shell, entry } = fakeShell(client);
    const wiring = createAssistantWiring(shell, { replaceWorkspace: vi.fn() });

    await wiring.send(client as unknown as FileManagerClient, entry.key, ' a backup script ');

    expect(seen[0]).toMatchObject({ requestId: 'req-1', profileId: 'p1', fileName: 'script.sh' });
    expect(seen[0]?.messages).toEqual([{ role: 'user', content: 'a backup script' }]);
    expect(entry.messages).toEqual([
      { role: 'user', content: 'a backup script' },
      { role: 'assistant', content: '#!/usr/bin/env bash\n' },
    ]);
    expect(entry.streamedText).toBeUndefined();
    expect(entry.pendingRequestId).toBeUndefined();
    expect(entry.error).toBeUndefined();
  });

  it('keeps the fragments that arrived while the reply was pending', async () => {
    const client = {
      streamFileAssistantReply: vi.fn(
        (_request: FileAssistantRequest, onEvent: (event: FileAssistantEvent) => void) => {
          onEvent({ type: 'delta', text: 'partial' });
          return new Promise<void>(() => undefined);
        },
      ),
    };
    const { shell, entry } = fakeShell(client);
    const wiring = createAssistantWiring(shell, { replaceWorkspace: vi.fn() });

    void wiring.send(client as unknown as FileManagerClient, entry.key, 'x');
    await Promise.resolve();
    expect(entry.streamedText).toBe('partial');
    expect(entry.pendingRequestId).toBe('req-1');
  });

  it('records a sanitized error when the reply fails', async () => {
    const client = {
      streamFileAssistantReply: vi.fn(
        (_request: FileAssistantRequest, onEvent: (event: FileAssistantEvent) => void) => {
          onEvent({ type: 'failed', reason: 'unavailable' });
          return Promise.resolve();
        },
      ),
    };
    const { shell, entry } = fakeShell(client);
    const wiring = createAssistantWiring(shell, { replaceWorkspace: vi.fn() });

    await wiring.send(client as unknown as FileManagerClient, entry.key, 'x');

    expect(entry.error).toBeTruthy();
    expect(entry.messages.map((message) => message.role)).toEqual(['user']);
    expect(entry.pendingRequestId).toBeUndefined();
  });

  it('records a sanitized error when the transport rejects', async () => {
    const client = {
      streamFileAssistantReply: vi.fn(() => Promise.reject(new Error('boom'))),
    };
    const { shell, entry } = fakeShell(client);
    const wiring = createAssistantWiring(shell, { replaceWorkspace: vi.fn() });

    await wiring.send(client as unknown as FileManagerClient, entry.key, 'x');

    expect(entry.error).toBeTruthy();
    expect(entry.error).not.toContain('boom');
    expect(entry.pendingRequestId).toBeUndefined();
  });

  it('stop cancels the request and discards its late events', async () => {
    let emit: (event: FileAssistantEvent) => void = () => undefined;
    const client = {
      streamFileAssistantReply: vi.fn(
        (_request: FileAssistantRequest, onEvent: (event: FileAssistantEvent) => void) => {
          emit = onEvent;
          return new Promise<void>(() => undefined);
        },
      ),
      cancelFileAssistantReply: vi.fn(() => Promise.resolve()),
    };
    const { shell, entry } = fakeShell(client);
    const wiring = createAssistantWiring(shell, { replaceWorkspace: vi.fn() });

    void wiring.send(client as unknown as FileManagerClient, entry.key, 'x');
    emit({ type: 'delta', text: 'half' });
    wiring.stop(client as unknown as FileManagerClient, entry.key);
    emit({ type: 'delta', text: ' more' });
    emit({ type: 'done', text: 'late' });

    expect(client.cancelFileAssistantReply).toHaveBeenCalledWith('req-1');
    expect(entry.streamedText).toBeUndefined();
    expect(entry.messages.map((message) => message.role)).toEqual(['user']);
  });

  it('insert places an assistant reply into the source editor only', () => {
    const { shell, entry, setContent } = fakeShell({});
    entry.messages = [
      { role: 'user', content: 'x' },
      { role: 'assistant', content: 'echo hi\n' },
    ];
    const wiring = createAssistantWiring(shell, { replaceWorkspace: vi.fn() });

    wiring.insert(entry.key, 0);
    expect(setContent).not.toHaveBeenCalled();
    wiring.insert(entry.key, 1);
    expect(setContent).toHaveBeenCalledWith('echo hi\n');
  });
});
