//! Transport types for the chat tab (task 0324): general conversations, saved per workspace, that
//! answer through the chat role (0322). The model may read context the person attached; it cannot
//! change files.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

/// Who wrote one chat message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum ChatRoleDto {
    /// Written by the person.
    User,
    /// Written by the model.
    Assistant,
}

/// One message of a saved conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessageDto {
    /// Author of the message.
    pub role: ChatRoleDto,
    /// Message text as the person wrote it, or as the model replied.
    pub content: String,
    /// Read-only context the person attached to this message, exactly as it was sent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
}

/// A saved conversation without its messages, for the conversation list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChatConversationSummaryDto {
    /// Stable identity of the conversation.
    pub id: Uuid,
    /// Title; empty until the person renames it or the first message sets it.
    pub title: String,
    /// When the conversation last changed, as an RFC 3339 timestamp.
    pub updated_at: String,
    /// Number of messages saved so far.
    pub message_count: u32,
}

/// A saved conversation with its messages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChatConversationDto {
    /// Stable identity of the conversation.
    pub id: Uuid,
    /// Title; empty until the person renames it or the first message sets it.
    pub title: String,
    /// When the conversation last changed, as an RFC 3339 timestamp.
    pub updated_at: String,
    /// Messages in order, oldest first.
    pub messages: Vec<ChatMessageDto>,
}

/// Renames a saved conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RenameChatConversationRequestDto {
    /// New title. Blank clears it.
    pub title: String,
}

/// Asks for the next reply in a saved conversation. The person's message is saved first; the
/// reply is saved only when it completes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(example = json!({
    "requestId": "5b1b6b1e-9b1b-4b1b-8b1b-1b1b1b1b1b1b",
    "workspaceId": "5b1b6b1e-9b1b-4b1b-8b1b-1b1b1b1b1b1c",
    "conversationId": "5b1b6b1e-9b1b-4b1b-8b1b-1b1b1b1b1b1d",
    "content": "Which of these folders is the largest?",
    "context": "Folder listing of /Users/me/notes (3 entries):\n- archive/ (folder)\n- todo.md (1.2 KB)"
}))]
pub struct ChatReplyRequestDto {
    /// Identity used to cancel this request.
    pub request_id: Uuid,
    /// Workspace that owns the conversation.
    pub workspace_id: Uuid,
    /// Conversation to continue.
    pub conversation_id: Uuid,
    /// The person's message.
    pub content: String,
    /// Read-only context attached to this message, exactly as shown to the person.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
}

/// Why a chat reply ended without text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum ChatFailureDto {
    /// The request violates a bounded contract.
    InvalidRequest,
    /// The conversation does not exist.
    NotFound,
    /// No endpoint is assigned to the chat role in Settings.
    RoleNotAssigned,
    /// The chat endpoint needs the person's consent in Settings before it can be used.
    ConsentRequired,
    /// The provider failed.
    Unavailable,
    /// The model returned no usable text.
    Empty,
    /// The person stopped the request.
    Cancelled,
}

/// One event of a streamed chat reply: fragments as they arrive, then exactly one final event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ChatReplyEventDto {
    /// A fragment of the reply, in arrival order. Fragments are provisional.
    Delta {
        /// Text to append to what has arrived so far.
        text: String,
    },
    /// The whole reply, saved with the conversation. It replaces the fragments.
    Done {
        /// Reply text.
        text: String,
    },
    /// The reply ended without text; no `done` event follows.
    Failed {
        /// Why the reply ended.
        reason: ChatFailureDto,
    },
}

/// Requests cancellation of one running chat reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CancelChatReplyRequestDto {
    /// Identity supplied when the reply was requested.
    pub request_id: Uuid,
}
