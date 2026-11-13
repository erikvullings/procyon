import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { FileManagerClient } from '../api/client/file-manager-client';
import type { ChatTabEntry } from '../features/chat/chat-tab-state';
import { t } from '../i18n';
import type { ChatReplyEvent, ChatReplyRequest } from '../models';
import { createChatWiring } from './chat-wiring';
import type { ShellState } from './shell-state';

function fakeShell(client: Partial<FileManagerClient>): { shell: ShellState; entry: ChatTabEntry } {
  const entry: ChatTabEntry = {
    key: 'left:t1',
    paneId: 'left',
    tabId: 't1',
    workspaceId: 'w1',
    conversations: [
      { id: 'c1', title: '', updatedAt: '2026-11-12T10:00:00.000Z', messageCount: 0 },
    ],
    conversationId: 'c1',
    messages: [],
    attachments: [],
    images: [],
  };
  const shell = {
    client: client as FileManagerClient,
    chatByTab: new Map([[entry.key, entry]]),
    chatOpening: false,
  } as unknown as ShellState;
  return { shell, entry };
}

beforeEach(() => {
  vi.stubGlobal('crypto', { randomUUID: () => 'req-1' });
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('chat wiring', () => {
  it('sends the attached context exactly as previewed and saves the reply', async () => {
    const requests: ChatReplyRequest[] = [];
    const client = {
      streamChatReply: vi.fn(
        (request: ChatReplyRequest, onEvent: (event: ChatReplyEvent) => void) => {
          requests.push(request);
          onEvent({ type: 'delta', text: 'Two' });
          onEvent({ type: 'done', text: 'Two files.' });
          return Promise.resolve();
        },
      ),
      listChatConversations: vi.fn(() => Promise.resolve([])),
    };
    const { shell, entry } = fakeShell(client);
    entry.attachments = [
      {
        id: 'a1',
        kind: 'folder',
        label: 'Folder listing',
        text: 'Folder listing of /notes:\n- a.txt',
      },
    ];
    const wiring = createChatWiring(shell, { replaceWorkspace: vi.fn() });

    await wiring.send(client as unknown as FileManagerClient, entry.key, ' How many? ');

    expect(requests[0]).toEqual({
      requestId: 'req-1',
      workspaceId: 'w1',
      conversationId: 'c1',
      content: 'How many?',
      context: 'Folder listing of /notes:\n- a.txt',
      retry: false,
    });
    expect(entry.attachments).toEqual([]);
    expect(entry.messages.map((message) => message.role)).toEqual(['user', 'assistant']);
    expect(entry.pendingRequestId).toBeUndefined();
  });

  it('reports a cloud endpoint without consent and never retries or prompts', async () => {
    const client = {
      streamChatReply: vi.fn(
        (_request: ChatReplyRequest, onEvent: (event: ChatReplyEvent) => void) => {
          onEvent({ type: 'failed', reason: 'consentRequired' });
          return Promise.resolve();
        },
      ),
      listChatConversations: vi.fn(() => Promise.resolve([])),
    };
    const { shell, entry } = fakeShell(client);
    const wiring = createChatWiring(shell, { replaceWorkspace: vi.fn() });

    await wiring.send(client as unknown as FileManagerClient, entry.key, 'Hi');

    expect(client.streamChatReply).toHaveBeenCalledTimes(1);
    expect(entry.failure).toBe('consentRequired');
    expect(entry.error).toBe(t('chat', 'consentRequired'));
    expect(entry.pendingRequestId).toBeUndefined();
    // The person's message stays in the conversation; no reply was saved.
    expect(entry.messages.map((message) => message.role)).toEqual(['user']);
  });

  it('stops a reply in flight: the backend is told, and later events are ignored', async () => {
    let deliver: ((event: ChatReplyEvent) => void) | undefined;
    const client = {
      streamChatReply: vi.fn(
        (_request: ChatReplyRequest, onEvent: (event: ChatReplyEvent) => void) => {
          deliver = onEvent;
          return new Promise<void>(() => undefined);
        },
      ),
      cancelChatReply: vi.fn(() => Promise.resolve()),
      listChatConversations: vi.fn(() => Promise.resolve([])),
    };
    const { shell, entry } = fakeShell(client);
    const wiring = createChatWiring(shell, { replaceWorkspace: vi.fn() });

    void wiring.send(client as unknown as FileManagerClient, entry.key, 'Long question');
    expect(entry.pendingRequestId).toBe('req-1');

    wiring.stop(client as unknown as FileManagerClient, entry.key);
    expect(client.cancelChatReply).toHaveBeenCalledWith('req-1');
    expect(entry.pendingRequestId).toBeUndefined();

    deliver?.({ type: 'done', text: 'too late' });
    expect(entry.messages.map((message) => message.role)).toEqual(['user']);
  });

  it('attaches the contents of one selected file, read only on request', async () => {
    const client = {
      loadEditableFile: vi.fn(() =>
        Promise.resolve({ content: 'buy milk\n', revision: 'r1', size: 10 }),
      ),
    };
    const { shell, entry } = fakeShell(client);
    shell.workspace = {
      paneOrder: ['left', 'right'],
      panesById: { left: { activeTabId: 't1' }, right: { activeTabId: 'f1' } },
    } as unknown as ShellState['workspace'];
    const folderKey = 'right:f1';
    shell.directories = new Map([
      [
        folderKey,
        {
          location: { providerId: 'local', uri: '/notes' },
          entries: [
            {
              id: 'e1',
              name: 'todo.md',
              kind: 'file',
              size: 10,
              location: { providerId: 'local', uri: '/notes/todo.md' },
            },
          ],
        },
      ],
    ]) as unknown as ShellState['directories'];
    shell.selections = new Map([
      [folderKey, { selectedEntryIds: ['e1'] }],
    ]) as unknown as ShellState['selections'];
    const wiring = createChatWiring(shell, { replaceWorkspace: vi.fn() });

    await wiring.attachFileContents(client as unknown as FileManagerClient, entry.key);

    expect(client.loadEditableFile).toHaveBeenCalledWith({
      location: { providerId: 'local', uri: '/notes/todo.md' },
    });
    expect(entry.attachments).toHaveLength(1);
    expect(entry.attachments[0]?.text).toBe('Contents of todo.md (10 B):\nbuy milk\n');
  });

  it('sends pasted images with the message and clears them once sent', async () => {
    const requests: ChatReplyRequest[] = [];
    const client = {
      streamChatReply: vi.fn(
        (request: ChatReplyRequest, onEvent: (event: ChatReplyEvent) => void) => {
          requests.push(request);
          onEvent({ type: 'done', text: 'A cat.' });
          return Promise.resolve();
        },
      ),
      listChatConversations: vi.fn(() => Promise.resolve([])),
    };
    const { shell, entry } = fakeShell(client);
    entry.images = [{ id: 'i1', data: 'QUJD', previewUrl: 'data:image/jpeg;base64,QUJD' }];
    const wiring = createChatWiring(shell, { replaceWorkspace: vi.fn() });

    await wiring.send(client as unknown as FileManagerClient, entry.key, 'What is this?');

    expect(requests[0]?.images).toEqual([{ data: 'QUJD' }]);
    expect(entry.images).toEqual([]);
  });

  it('refuses pasted images unless the chat endpoint is marked as accepting them', async () => {
    const client = {
      listLlmRoles: vi.fn(() =>
        Promise.resolve({ chat: { profileId: 'p1', model: 'm', allowCloud: false } }),
      ),
      listLlmProfiles: vi.fn(() =>
        Promise.resolve([{ id: 'p1', capabilities: ['chatCompletions'] }]),
      ),
    };
    const { shell, entry } = fakeShell(client as unknown as Partial<FileManagerClient>);
    const wiring = createChatWiring(shell, { replaceWorkspace: vi.fn() });

    await wiring.pasteImages(client as unknown as FileManagerClient, entry.key, [
      new Blob(['x'], { type: 'image/png' }),
    ]);

    expect(entry.images).toEqual([]);
  });

  it('retries a failed reply without sending the message again', async () => {
    const requests: ChatReplyRequest[] = [];
    const client = {
      streamChatReply: vi.fn(
        (request: ChatReplyRequest, onEvent: (event: ChatReplyEvent) => void) => {
          requests.push(request);
          if (requests.length === 1) {
            onEvent({ type: 'delta', text: 'Partly' });
            onEvent({ type: 'failed', reason: 'unavailable' });
          } else {
            onEvent({ type: 'done', text: 'Whole.' });
          }
          return Promise.resolve();
        },
      ),
      listChatConversations: vi.fn(() => Promise.resolve([])),
    };
    const { shell, entry } = fakeShell(client);
    const wiring = createChatWiring(shell, { replaceWorkspace: vi.fn() });

    await wiring.send(client as unknown as FileManagerClient, entry.key, 'Question');
    expect(entry.partialReply).toBe('Partly');
    await wiring.retry(client as unknown as FileManagerClient, entry.key);

    expect(requests[1]).toMatchObject({ retry: true, content: '' });
    expect(entry.messages.map((message) => message.role)).toEqual(['user', 'assistant']);
    expect(entry.partialReply).toBeUndefined();
  });

  it('attaches nothing when the other pane shows no folder', () => {
    const { shell, entry } = fakeShell({});
    const wiring = createChatWiring(shell, { replaceWorkspace: vi.fn() });
    expect(wiring.folderAvailable(entry.key)).toBe(false);
    expect(wiring.attachFolder(entry.key)).toBeUndefined();
    expect(entry.attachments).toEqual([]);
  });
});
