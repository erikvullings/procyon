import type {
  ChatConversationDto,
  ChatConversationSummaryDto,
  ChatFailureDto,
  ChatMessageDto,
  ChatReplyEventDto,
  ChatReplyRequestDto,
  ChatRoleDto,
  RenameChatConversationRequestDto,
} from '../api/generated/models';

/** Saved chat conversations and replies (task 0324). Conversations stay on this machine. */
export type {
  ChatConversationDto as ChatConversation,
  ChatConversationSummaryDto as ChatConversationSummary,
  ChatFailureDto as ChatFailure,
  ChatMessageDto as ChatMessage,
  ChatReplyEventDto as ChatReplyEvent,
  ChatReplyRequestDto as ChatReplyRequest,
  ChatRoleDto as ChatRole,
  RenameChatConversationRequestDto as RenameChatConversationRequest,
};
