import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { ChatConversationSummary } from '../../models';
import { ChatTab, type ChatTabAttrs } from './chat-tab';
import type { ChatTabEntry } from './chat-tab-state';

// The header row is the pane's breadcrumb bar (task 0324), so its buttons are part of its contract.
const conversations: ChatConversationSummary[] = [
  { id: 'c1', title: 'Naming a folder', updatedAt: '2026-11-15T09:00:00.000Z', messageCount: 4 },
];

const entry = (): ChatTabEntry => ({
  key: 'k',
  paneId: 'p' as never,
  tabId: 't' as never,
  workspaceId: 'w',
  conversations,
  conversationId: 'c1',
  messages: [],
  attachments: [],
  images: [],
});

const noop = () => undefined;

describe('ChatTab header', () => {
  let root: HTMLElement;

  beforeEach(() => {
    root = document.createElement('div');
    document.body.appendChild(root);
  });

  afterEach(() => {
    m.render(root, null);
    root.remove();
  });

  function render(overrides: Partial<ChatTabAttrs> = {}): void {
    m.render(
      root,
      m(ChatTab, {
        entry: entry(),
        folderAvailable: false,
        folderSelection: { selected: 0, selectedFiles: 0, fileTooLarge: false },
        onNew: noop,
        onSelect: noop,
        onRename: noop,
        onDelete: noop,
        onFork: noop,
        onSend: noop,
        onStop: noop,
        onRetry: noop,
        onToggleTools: noop,
        onThinking: noop,
        onAttachFolder: () => undefined,
        onAttachSelection: () => undefined,
        onAttachFileContents: noop,
        onDetach: noop,
        images: [],
        onPrepareImagePaste: noop,
        onPasteImages: noop,
        onRemoveImage: noop,
        ...overrides,
      }),
    );
    m.redraw();
  }

  function headerLabels(): string[] {
    return [...root.querySelectorAll('.fm-chat-header .fm-chat-chrome-icon')].map(
      (node) => node.getAttribute('aria-label') ?? '',
    );
  }

  it('routes image items to attachment handling and keeps native text paste available', () => {
    const onPrepareImagePaste = vi.fn();
    const onPasteImages = vi.fn();
    render({ onPrepareImagePaste, onPasteImages });
    const input = root.querySelector('textarea') as HTMLTextAreaElement;
    input.focus();
    expect(onPrepareImagePaste).toHaveBeenCalledTimes(1);

    const image = new File(['image'], 'clipboard.png', { type: 'image/png' });
    const textItem = { kind: 'string', type: 'text/plain', getAsFile: () => null };
    const imageItem = { kind: 'file', type: 'image/png', getAsFile: () => image };
    for (const items of [[textItem], [textItem, imageItem]]) {
      const event = new Event('paste', { bubbles: true, cancelable: true });
      Object.defineProperty(event, 'clipboardData', {
        value: { items, getData: () => 'Short clipboard text' },
      });
      expect(input.dispatchEvent(event)).toBe(true);
    }
    expect(onPasteImages).toHaveBeenCalledExactlyOnceWith([image]);
  });

  it('offers a new chat, the AI settings and a new tab, in that order', () => {
    render({ onNewTab: noop, onOpenSettings: noop });
    expect(headerLabels()).toEqual(['New chat', 'Open AI settings', 'New tab']);
  });

  it('drops the favourites heart, which has no meaning beside a chat', () => {
    render({ onNewTab: noop, onOpenSettings: noop });
    expect(root.querySelector('[aria-label="Favourites"]')).toBeNull();
  });

  it('opens the AI settings from the header', () => {
    const onOpenSettings = vi.fn();
    render({ onOpenSettings });
    (root.querySelector('.fm-chat-header [aria-label="Open AI settings"]') as HTMLElement).click();
    expect(onOpenSettings).toHaveBeenCalledTimes(1);
  });

  it('offers no settings button where Settings cannot be opened', () => {
    render({ onNewTab: noop });
    expect(root.querySelector('.fm-chat-header [aria-label="Open AI settings"]')).toBeNull();
  });
});

describe('ChatTab fork from here', () => {
  let root: HTMLElement;

  beforeEach(() => {
    root = document.createElement('div');
    document.body.appendChild(root);
  });

  afterEach(() => {
    m.render(root, null);
    root.remove();
  });

  function render(onFork: (keep: number) => void): void {
    m.render(
      root,
      m(ChatTab, {
        entry: {
          ...entry(),
          messages: [
            { role: 'user', content: 'Which files are large?' },
            { role: 'assistant', content: 'The ISO.' },
          ],
        },
        folderAvailable: false,
        folderSelection: { selected: 0, selectedFiles: 0, fileTooLarge: false },
        onNew: noop,
        onSelect: noop,
        onRename: noop,
        onDelete: noop,
        onFork,
        onSend: noop,
        onStop: noop,
        onRetry: noop,
        onToggleTools: noop,
        onThinking: noop,
        onAttachFolder: () => undefined,
        onAttachSelection: () => undefined,
        onAttachFileContents: noop,
        onDetach: noop,
        images: [],
        onPrepareImagePaste: noop,
        onPasteImages: noop,
        onRemoveImage: noop,
      }),
    );
  }

  it('chooses a thinking effort for the chat, or goes back to the default', () => {
    const onThinking = vi.fn();
    m.render(
      root,
      m(ChatTab, {
        entry: entry(),
        folderAvailable: false,
        folderSelection: { selected: 0, selectedFiles: 0, fileTooLarge: false },
        onNew: noop,
        onSelect: noop,
        onRename: noop,
        onDelete: noop,
        onFork: noop,
        onSend: noop,
        onStop: noop,
        onRetry: noop,
        onToggleTools: noop,
        onThinking,
        onAttachFolder: () => undefined,
        onAttachSelection: () => undefined,
        onAttachFileContents: noop,
        onDetach: noop,
        images: [],
        onPrepareImagePaste: noop,
        onPasteImages: noop,
        onRemoveImage: noop,
      }),
    );
    const select = root.querySelector(
      'select[aria-label="Thinking effort for this chat"]',
    ) as HTMLSelectElement;
    expect([...select.options].map((option) => option.value)).toEqual([
      '',
      'off',
      'low',
      'medium',
      'high',
    ]);
    select.value = 'high';
    select.dispatchEvent(new Event('change'));
    expect(onThinking).toHaveBeenLastCalledWith('high');
    select.value = '';
    select.dispatchEvent(new Event('change'));
    expect(onThinking).toHaveBeenLastCalledWith(undefined);
  });

  it('forks before a question and puts it in the draft, or up to and with a reply', () => {
    const onFork = vi.fn();
    render(onFork);
    const question = root.querySelector(
      '[aria-label="Fork: new chat from before this question"]',
    ) as HTMLButtonElement;
    question.click();
    expect(onFork).toHaveBeenLastCalledWith(0);
    render(onFork);
    expect((root.querySelector('textarea') as HTMLTextAreaElement).value).toBe(
      'Which files are large?',
    );
    (
      root.querySelector('[aria-label="Fork: new chat up to this reply"]') as HTMLButtonElement
    ).click();
    expect(onFork).toHaveBeenLastCalledWith(2);
  });
});
