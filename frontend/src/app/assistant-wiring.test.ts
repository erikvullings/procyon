import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { FileManagerClient } from '../api/client/file-manager-client';
import type { AssistantTabEntry } from '../features/assistant/assistant-tab-state';
import type { FileAssistantRequest, LlmProfile } from '../models';
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
  it('appends the user message and the reply, then clears the pending request', async () => {
    const seen: FileAssistantRequest[] = [];
    const client = {
      draftFileAssistantReply: vi.fn((request: FileAssistantRequest) => {
        seen.push(request);
        return Promise.resolve({ text: '#!/usr/bin/env bash\n' });
      }),
    };
    const { shell, entry } = fakeShell(client);
    const wiring = createAssistantWiring(shell, { replaceWorkspace: vi.fn() });

    await wiring.send(client as unknown as FileManagerClient, entry.key, ' a backup script ');

    expect(seen[0]).toMatchObject({ requestId: 'req-1', profileId: 'p1', fileName: 'script.sh' });
    expect(seen[0]?.messages).toEqual([{ role: 'user', content: 'a backup script' }]);
    expect(entry.messages.map((message) => message.role)).toEqual(['user', 'assistant']);
    expect(entry.pendingRequestId).toBeUndefined();
    expect(entry.error).toBeUndefined();
  });

  it('records a sanitized error when the reply fails', async () => {
    const client = {
      draftFileAssistantReply: vi.fn(() => Promise.reject(new Error('boom'))),
    };
    const { shell, entry } = fakeShell(client);
    const wiring = createAssistantWiring(shell, { replaceWorkspace: vi.fn() });

    await wiring.send(client as unknown as FileManagerClient, entry.key, 'x');

    expect(entry.error).toBeTruthy();
    expect(entry.error).not.toContain('boom');
    expect(entry.pendingRequestId).toBeUndefined();
  });

  it('stop cancels the request and discards its late reply', async () => {
    let resolveReply: (value: { text: string }) => void = () => undefined;
    const client = {
      draftFileAssistantReply: vi.fn(
        () => new Promise<{ text: string }>((resolve) => (resolveReply = resolve)),
      ),
      cancelFileAssistantReply: vi.fn(() => Promise.resolve()),
    };
    const { shell, entry } = fakeShell(client);
    const wiring = createAssistantWiring(shell, { replaceWorkspace: vi.fn() });

    const sending = wiring.send(client as unknown as FileManagerClient, entry.key, 'x');
    wiring.stop(client as unknown as FileManagerClient, entry.key);
    resolveReply({ text: 'late' });
    await sending;

    expect(client.cancelFileAssistantReply).toHaveBeenCalledWith('req-1');
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
