import { describe, expect, it } from 'vitest';
import {
  type ChatAttachment,
  type ChatTabEntry,
  canSendMessage,
  chatFolderPaneId,
  chatPlacementPaneId,
  contextForMessage,
  failureNeedsSettings,
} from './chat-tab-state';

const entry = (overrides: Partial<ChatTabEntry> = {}): ChatTabEntry => ({
  key: 'right:t1',
  paneId: 'right',
  tabId: 't1',
  workspaceId: 'w1',
  conversations: [],
  messages: [],
  attachments: [],
  conversationId: 'c1',
  ...overrides,
});

const attachment = (id: string, text: string): ChatAttachment => ({ id, label: id, text });

describe('chat placement', () => {
  it('opens in the active pane when it shows no editor', () => {
    expect(chatPlacementPaneId(['left', 'right'], 'left', false)).toBe('left');
  });

  it('opens in the opposite pane when the active pane shows an editor', () => {
    expect(chatPlacementPaneId(['left', 'right'], 'left', true)).toBe('right');
    expect(chatPlacementPaneId(['left', 'right'], 'right', true)).toBe('left');
  });

  it('stays in the active pane when there is no other pane', () => {
    expect(chatPlacementPaneId(['left'], 'left', true)).toBe('left');
  });

  it('attaches the folder of the other pane only', () => {
    expect(chatFolderPaneId(['left', 'right'], 'right')).toBe('left');
    expect(chatFolderPaneId(['left'], 'left')).toBeUndefined();
  });
});

describe('attach preview', () => {
  it('sends exactly the attached texts in order, and nothing when none are attached', () => {
    expect(contextForMessage([])).toBeNull();
    expect(
      contextForMessage([attachment('a', 'Folder listing:\n- a.txt'), attachment('b', 'Selected')]),
    ).toBe('Folder listing:\n- a.txt\n\nSelected');
  });
});

describe('sending', () => {
  it('needs a conversation, a non-blank draft, and no reply in flight', () => {
    expect(canSendMessage(entry(), 'hello')).toBe(true);
    expect(canSendMessage(entry(), '   ')).toBe(false);
    expect(canSendMessage(entry({ conversationId: undefined }), 'hello')).toBe(false);
    expect(canSendMessage(entry({ pendingRequestId: 'r1' }), 'hello')).toBe(false);
  });

  it('links to Settings only for failures fixed there', () => {
    expect(failureNeedsSettings('consentRequired')).toBe(true);
    expect(failureNeedsSettings('roleNotAssigned')).toBe(true);
    expect(failureNeedsSettings('unavailable')).toBe(false);
    expect(failureNeedsSettings(undefined)).toBe(false);
  });
});
