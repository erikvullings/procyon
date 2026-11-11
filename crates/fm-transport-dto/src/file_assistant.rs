//! Transport types for the file assistant (task 0321): a chat that drafts the contents of one new
//! text file through a saved LLM profile.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

/// Who wrote one chat message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum FileAssistantRoleDto {
    /// Written by the person.
    User,
    /// Written by the model.
    Assistant,
}

/// One chat message in a file-assistant conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FileAssistantMessageDto {
    /// Author of the message.
    pub role: FileAssistantRoleDto,
    /// Message text.
    pub content: String,
}

/// Asks the file assistant for the next reply in a conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(example = json!({
    "requestId": "5b1b6b1e-9b1b-4b1b-8b1b-1b1b1b1b1b1b",
    "profileId": "5b1b6b1e-9b1b-4b1b-8b1b-1b1b1b1b1b1c",
    "fileName": "backup.sh",
    "messages": [{"role": "user", "content": "A script that archives ~/notes into a dated tarball"}]
}))]
pub struct FileAssistantRequestDto {
    /// Identity used to cancel this request.
    pub request_id: Uuid,
    /// Saved LLM profile that generates the reply.
    pub profile_id: Uuid,
    /// Name of the file being written. Only the name is sent to the model, never its contents.
    pub file_name: String,
    /// The conversation so far, ending with the person's latest message.
    pub messages: Vec<FileAssistantMessageDto>,
}

/// The model's reply, with the file contents already unwrapped from any single code fence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FileAssistantReplyDto {
    /// Reply text, to be placed in the editor when the person accepts it.
    pub text: String,
}

/// Requests cancellation of one running file-assistant request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CancelFileAssistantRequestDto {
    /// Identity supplied when the request was started.
    pub request_id: Uuid,
}
