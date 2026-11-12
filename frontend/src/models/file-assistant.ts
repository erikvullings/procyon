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

/** Why a reply ended without text. `cancelled` means the person stopped it. */
export type FileAssistantFailure = 'invalidRequest' | 'unavailable' | 'empty' | 'cancelled';

/**
 * One streamed event: fragments as they arrive, then exactly one `done` (the whole reply, ready to
 * place in the editor) or one `failed`.
 */
export type FileAssistantEvent =
  | { readonly type: 'delta'; readonly text: string }
  | { readonly type: 'done'; readonly text: string }
  | { readonly type: 'failed'; readonly reason: FileAssistantFailure };
