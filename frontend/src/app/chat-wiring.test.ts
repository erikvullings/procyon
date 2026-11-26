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
      // Reading public pages is on unless the setting turns it off (task 0363).
      allowWeb: true,
    });
    expect(entry.attachments).toEqual([]);
    expect(entry.messages.map((message) => message.role)).toEqual(['user', 'assistant']);
    expect(entry.pendingRequestId).toBeUndefined();
  });

  it('shows what a long tool call reports until the next event, and never saves it', async () => {
    const seen: (string | undefined)[] = [];
    let entryRef: ChatTabEntry | undefined;
    const client = {
      streamChatReply: vi.fn(
        (_request: ChatReplyRequest, onEvent: (event: ChatReplyEvent) => void) => {
          onEvent({ type: 'progress', text: 'Summarising a.pdf: part 1 of 3' });
          seen.push(entryRef?.progress);
          onEvent({ type: 'progress', text: 'Summarising a.pdf: part 2 of 3' });
          seen.push(entryRef?.progress);
          onEvent({ type: 'step', text: 'Summarised a.pdf (3 parts)' });
          seen.push(entryRef?.progress);
          onEvent({ type: 'done', text: 'Summary.' });
          return Promise.resolve();
        },
      ),
      listChatConversations: vi.fn(() => Promise.resolve([])),
    };
    const { shell, entry } = fakeShell(client);
    entryRef = entry;
    const wiring = createChatWiring(shell, { replaceWorkspace: vi.fn() });

    await wiring.send(client as unknown as FileManagerClient, entry.key, 'Summarise a.pdf');

    expect(seen).toEqual([
      'Summarising a.pdf: part 1 of 3',
      'Summarising a.pdf: part 2 of 3',
      undefined,
    ]);
    expect(entry.progress).toBeUndefined();
    expect(entry.messages[entry.messages.length - 1]).toMatchObject({
      role: 'assistant',
      steps: ['Summarised a.pdf (3 parts)'],
    });
  });

  it('sends the folder only when tools are on, shows the steps, and reviews a proposed note', async () => {
    const requests: ChatReplyRequest[] = [];
    const proposal = { draft: { title: 'T', content: 'C' } };
    const client = {
      streamChatReply: vi.fn(
        (request: ChatReplyRequest, onEvent: (event: ChatReplyEvent) => void) => {
          requests.push(request);
          onEvent({ type: 'step', text: 'Read a.txt' });
          onEvent({ type: 'noteProposal', proposal } as unknown as ChatReplyEvent);
          onEvent({ type: 'done', text: 'Drafted.' });
          return Promise.resolve();
        },
      ),
      listChatConversations: vi.fn(() => Promise.resolve([])),
    };
    const { shell, entry } = fakeShell(client);
    const showProposal = vi.fn(() => Promise.resolve());
    Object.assign(shell, {
      notesController: { showProposal },
      workspace: {
        paneOrder: ['left', 'right'],
        panesById: { right: { activeTabId: 'r1' } },
      },
      directories: new Map([['right:r1', { location: { uri: 'file:///project/' }, entries: [] }]]),
    });
    const wiring = createChatWiring(shell, { replaceWorkspace: vi.fn() });

    await wiring.send(client as unknown as FileManagerClient, entry.key, 'Off');
    expect(requests[0]?.toolsFolder).toBeUndefined();

    wiring.toggleTools(entry.key);
    expect(entry.useTools).toBe(true);
    await wiring.send(client as unknown as FileManagerClient, entry.key, 'On');

    expect(requests[1]?.toolsFolder).toBe('file:///project/');
    // The lookups move onto the reply, which shows them (and the backend saves them with it).
    expect(entry.steps).toEqual([]);
    expect(entry.messages[entry.messages.length - 1]).toMatchObject({
      role: 'assistant',
      steps: ['Read a.txt'],
    });
    expect(showProposal).toHaveBeenCalledWith(proposal);
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
    entry.images = [
      {
        id: 'i1',
        data: 'QUJD',
        mediaType: 'image/jpeg',
        previewUrl: 'data:image/jpeg;base64,QUJD',
      },
    ];
    const wiring = createChatWiring(shell, { replaceWorkspace: vi.fn() });

    await wiring.send(client as unknown as FileManagerClient, entry.key, 'What is this?');

    expect(requests[0]?.images).toEqual([{ data: 'QUJD', mediaType: 'image/jpeg' }]);
    expect(entry.images).toEqual([]);
  });

  it('checks vision before paste, blocks send during the check and refuses images without vision', async () => {
    const encoding = vi.fn();
    vi.stubGlobal('createImageBitmap', encoding);
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

    const fileManager = client as unknown as FileManagerClient;
    wiring.prepareImagePaste(fileManager);
    expect(client.listLlmRoles).toHaveBeenCalledTimes(1);
    const paste = wiring.pasteImages(fileManager, entry.key, [
      new Blob(['x'], { type: 'image/png' }),
    ]);
    await wiring.send(fileManager, entry.key, 'Look');
    await paste;

    expect(client.listLlmRoles).toHaveBeenCalledTimes(1);
    expect(encoding).not.toHaveBeenCalled();
    expect(entry.messages).toEqual([]);
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

  it('sends the chosen thinking effort and restores it when the conversation opens', async () => {
    const requests: ChatReplyRequest[] = [];
    const client = {
      streamChatReply: vi.fn(
        (request: ChatReplyRequest, onEvent: (event: ChatReplyEvent) => void) => {
          requests.push(request);
          onEvent({ type: 'done', text: 'Done.' });
          return Promise.resolve();
        },
      ),
      listChatConversations: vi.fn(() => Promise.resolve([])),
      getChatConversation: vi.fn(() =>
        Promise.resolve({
          id: 'c2',
          title: 'Deep',
          updatedAt: '2026-11-25T10:00:00.000Z',
          messages: [],
          thinking: 'high' as const,
        }),
      ),
    };
    const { shell, entry } = fakeShell(client);
    const wiring = createChatWiring(shell, { replaceWorkspace: vi.fn() });
    wiring.setThinking(entry.key, 'low');
    await wiring.send(client as unknown as FileManagerClient, entry.key, 'Quick one');
    expect(requests[0]?.thinking).toBe('low');

    wiring.select(client as unknown as FileManagerClient, entry.key, 'c2');
    await vi.waitFor(() => expect(entry.conversationId).toBe('c2'));
    await vi.waitFor(() => expect(entry.thinking).toBe('high'));
  });

  it('attaches nothing when the other pane shows no folder', () => {
    const { shell, entry } = fakeShell({});
    const wiring = createChatWiring(shell, { replaceWorkspace: vi.fn() });
    expect(wiring.folderAvailable(entry.key)).toBe(false);
    expect(wiring.attachFolder(entry.key)).toBeUndefined();
    expect(entry.attachments).toEqual([]);
  });
});
