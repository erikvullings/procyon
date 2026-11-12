import type {
  ChatConversationSummary,
  ChatFailure,
  ChatMessage,
  PaneId,
  TabId,
} from '../../models';

/** Read-only material the person attached to the next message. `text` is exactly what is sent. */
export interface ChatAttachment {
  readonly id: string;
  /** Short name shown in the attach list. */
  readonly label: string;
  readonly text: string;
}

/** A chat tab (task 0324). Conversations and messages are mirrored from the backend. */
export interface ChatTabEntry {
  key: string;
  paneId: PaneId;
  tabId: TabId;
  readonly workspaceId: string;
  conversations: ChatConversationSummary[];
  conversationId?: string | undefined;
  messages: ChatMessage[];
  attachments: ChatAttachment[];
  pendingRequestId?: string | undefined;
  streamedText?: string | undefined;
  /** Why the last reply failed; drives the message and the link to Settings. */
  failure?: ChatFailure | undefined;
  error?: string | undefined;
}

/**
 * Where a new chat opens (task 0321 rule): beside the editor when the active pane shows one, in the
 * active pane otherwise.
 */
export function chatPlacementPaneId(
  paneOrder: readonly PaneId[],
  activePaneId: PaneId,
  activePaneShowsEditor: boolean,
): PaneId {
  if (!activePaneShowsEditor) return activePaneId;
  return paneOrder.find((candidate) => candidate !== activePaneId) ?? activePaneId;
}

/** The pane whose folder a chat can attach: the other pane, when there is one. */
export function chatFolderPaneId(
  paneOrder: readonly PaneId[],
  chatPaneId: PaneId,
): PaneId | undefined {
  return paneOrder.find((candidate) => candidate !== chatPaneId);
}

/** The context sent with the next message: every attachment, in order; `null` when none. */
export function contextForMessage(attachments: readonly ChatAttachment[]): string | null {
  return attachments.length === 0
    ? null
    : attachments.map((attachment) => attachment.text).join('\n\n');
}

/** Whether the person can send now: a conversation is open, the draft is not blank, nothing runs. */
export function canSendMessage(entry: ChatTabEntry, draft: string): boolean {
  return (
    draft.trim() !== '' &&
    entry.conversationId !== undefined &&
    entry.pendingRequestId === undefined
  );
}

/** Whether a failure is fixed in Settings, so the chat links there. */
export function failureNeedsSettings(failure: ChatFailure | undefined): boolean {
  return failure === 'consentRequired' || failure === 'roleNotAssigned';
}
