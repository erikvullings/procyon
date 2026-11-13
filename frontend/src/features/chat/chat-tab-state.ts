import type {
  ChatConversationSummary,
  ChatFailure,
  ChatMessage,
  PaneId,
  TabId,
} from '../../models';
import type { PastedImage } from './chat-image';

/** Read-only material the person attached to the next message. `text` is exactly what is sent. */
export interface ChatAttachment {
  readonly id: string;
  /** What was attached; a file's contents need the single-use guard in the attach command. */
  readonly kind: 'folder' | 'selection' | 'file';
  /** Short name shown in the pill. */
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
  /** Images pasted for the next message; sent with it, never saved in the conversation. */
  images: PastedImage[];
  pendingRequestId?: string | undefined;
  streamedText?: string | undefined;
  /** The text a failed reply had streamed before it stopped; kept on screen for the person. */
  partialReply?: string | undefined;
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

/** Pastes this long (lines or characters) show as a placeholder; shorter ones go in as typed. */
export const PASTE_PLACEHOLDER_MIN_LINES = 4;
export const PASTE_PLACEHOLDER_MIN_CHARS = 400;

/** A long paste held back behind its placeholder in the draft until the message is sent. */
export interface PastedText {
  readonly placeholder: string;
  readonly text: string;
}

export function isLongPaste(text: string): boolean {
  return (
    text.split('\n').length >= PASTE_PLACEHOLDER_MIN_LINES ||
    text.length >= PASTE_PLACEHOLDER_MIN_CHARS
  );
}

/** The placeholder shown in the draft for a paste, numbered so two pastes never collide. */
export function pastePlaceholder(text: string, number: number): string {
  return `[${text.split('\n').length} lines pasted #${number}]`;
}

/** Puts each paste back in place of its placeholder; a placeholder the person deleted is dropped. */
export function expandPastes(draft: string, pastes: readonly PastedText[]): string {
  return pastes.reduce(
    (current, paste) => current.replace(paste.placeholder, () => paste.text),
    draft,
  );
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
