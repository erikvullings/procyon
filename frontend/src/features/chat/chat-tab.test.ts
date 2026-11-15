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
        onSend: noop,
        onStop: noop,
        onRetry: noop,
        onAttachFolder: () => undefined,
        onAttachSelection: () => undefined,
        onAttachFileContents: noop,
        onDetach: noop,
        images: [],
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
