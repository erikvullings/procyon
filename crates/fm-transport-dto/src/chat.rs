//! Transport types for the chat tab (task 0324): general conversations, saved per workspace, that
//! answer through the chat role (0322). The model may read context the person attached; it cannot
//! change files.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::NoteProposalDto;

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
    /// What the tools looked up while the model wrote this reply, one line each.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub steps: Vec<String>,
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
    /// The thinking effort the conversation was last asked with (task 0361); absent uses the chat
    /// role's default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking: Option<crate::ThinkingEffortDto>,
}

/// Renames a saved conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RenameChatConversationRequestDto {
    /// New title. Blank clears it.
    pub title: String,
}

/// Starts a new conversation from the start of a saved one ("fork from here", task 0359).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ForkChatConversationRequestDto {
    /// How many leading messages the new conversation keeps; at most the conversation's length.
    pub keep: u32,
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
    /// Images pasted into this message, sent with it and not saved in the conversation. Empty
    /// unless the chat endpoint accepts images.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<ChatImageDto>,
    /// Asks again for the reply to the conversation's last message, which must be the person's.
    /// The message is not saved a second time, and `content` and `context` are not used.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub retry: bool,
    /// A folder (a location URL) the model may look into with read-only tools, together with the
    /// person's notes and skills (task 0339). It cannot leave the folder or change anything.
    /// Without it the model only answers from the conversation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools_folder: Option<String>,
    /// Whether the model may read public web pages with `web_fetch` (task 0363).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub allow_web: bool,
    /// The conversation's thinking effort (task 0361); absent uses the chat role's default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking: Option<crate::ThinkingEffortDto>,
}

/// One PNG, JPEG or WebP image sent with a chat message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChatImageDto {
    /// The declared MIME type. Older clients may omit it and let the server detect it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
    /// The encoded image, base64 without a data-URL prefix.
    pub data: String,
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
    /// The chat endpoint is not marked as accepting images in Settings.
    ImagesNotSupported,
    /// The model did not answer in time; a long conversation makes a local model slow to start.
    Timeout,
    /// The model stopped at its output limit before finishing its response.
    ReplyCutOff,
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
    /// A tool ran while the model worked toward its reply (task 0339).
    Step {
        /// One line saying what was looked up, such as `Read notes.md`.
        text: String,
    },
    /// What a long tool call is doing now, such as `Summarising a.pdf: part 2 of 5`. Shown until
    /// the next event; not saved with the reply.
    Progress {
        /// The one-line description.
        text: String,
    },
    /// The model drafted a note for the person to review. Nothing is saved unless they save it.
    NoteProposal {
        /// The proposed note.
        proposal: NoteProposalDto,
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
