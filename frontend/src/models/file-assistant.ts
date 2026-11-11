/** Who wrote one file-assistant message (task 0321). */
export type FileAssistantRole = 'user' | 'assistant';

/** One message of a file-assistant conversation. */
export interface FileAssistantMessage {
  readonly role: FileAssistantRole;
  readonly content: string;
}

/** A request for the next file-assistant reply. Only the file name and messages are sent. */
export interface FileAssistantRequest {
  /** Identity used to cancel this request. */
  readonly requestId: string;
  readonly profileId: string;
  readonly fileName: string;
  readonly messages: readonly FileAssistantMessage[];
}

/** The model's reply: the text of the file, ready to place in the editor. */
export interface FileAssistantReply {
  readonly text: string;
}
