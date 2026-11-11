import type {
  FileAssistantMessage,
  FileAssistantRole,
  LlmProfile,
  PaneId,
  TabId,
} from '../../models';

/** One message shown in the assistant chat. */
export interface AssistantChatMessage {
  readonly role: FileAssistantRole;
  readonly content: string;
}

/**
 * A file-assistant chat tab (task 0321). It sits in the pane opposite the editor it drafts for;
 * the reply is placed into that editor only when the person accepts it.
 */
export interface AssistantTabEntry {
  key: string;
  paneId: PaneId;
  tabId: TabId;
  /** The pane whose editor receives an accepted reply. */
  readonly sourcePaneId: PaneId;
  /** The name of the file being drafted. Only this name is sent to the model. */
  readonly fileName: string;
  readonly profiles: readonly LlmProfile[];
  /** The selected profile; empty when none is usable. */
  profileId: string;
  messages: AssistantChatMessage[];
  /** Identity of the request in flight, if any. */
  pendingRequestId?: string | undefined;
  error?: string | undefined;
}

/** The pane that is not `paneId`, in workspace order: where the chat opens. */
export function oppositePaneId(paneOrder: readonly PaneId[], paneId: PaneId): PaneId | undefined {
  return paneOrder.find((candidate) => candidate !== paneId);
}

/** The conversation as the backend expects it. */
export function transcriptForRequest(
  messages: readonly AssistantChatMessage[],
): FileAssistantMessage[] {
  return messages.map(({ role, content }) => ({ role, content }));
}

/** Whether the person's draft can be sent now. */
export function canSendDraft(entry: AssistantTabEntry, draft: string): boolean {
  return draft.trim() !== '' && entry.profileId !== '' && entry.pendingRequestId === undefined;
}
