//! Saved chat conversations (task 0324): a versioned document in the settings data root, grouped
//! by workspace. Conversations never leave the machine; only a reply request sends their messages
//! to the chat endpoint, and that is done by [`crate::chat`].

use std::collections::BTreeMap;
use std::sync::Mutex;

use chrono::{SecondsFormat, Utc};
use fm_settings::{SettingsStore, VersionedDocument};
use fm_transport_dto::{
    ChatConversationDto, ChatConversationSummaryDto, ChatMessageDto, ChatRoleDto,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use uuid::Uuid;

use crate::error::ApplicationError;

/// Schema of the conversations document written by this build.
const DOCUMENT_SCHEMA_VERSION: u32 = 1;
/// Longest title, in characters.
pub(crate) const MAX_TITLE_CHARS: usize = 200;
/// Characters of the first message that become the title when none was set.
const AUTO_TITLE_CHARS: usize = 60;

/// Failure of a conversation operation.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub(crate) enum ChatConversationError {
    /// No conversation with this id exists in the workspace.
    #[error("chat conversation not found")]
    NotFound,
    /// The change violates a bounded contract.
    #[error("invalid chat conversation: {0}")]
    Invalid(&'static str),
    /// The document could not be read or written.
    #[error("chat conversations could not be saved")]
    Persistence,
}

impl From<ChatConversationError> for ApplicationError {
    fn from(error: ChatConversationError) -> Self {
        match error {
            ChatConversationError::NotFound => Self::NotFound,
            ChatConversationError::Invalid(message) => Self::InvalidRequest(message.to_owned()),
            ChatConversationError::Persistence => Self::Internal,
        }
    }
}

/// Who wrote one saved message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ChatRole {
    User,
    Assistant,
}

/// One saved message. `context` is the read-only material attached to it, kept exactly as the
/// person saw it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
}

/// One saved conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChatConversation {
    pub id: Uuid,
    pub title: String,
    /// RFC 3339 with millisecond precision, so text order matches time order.
    pub updated_at: String,
    #[serde(default)]
    pub messages: Vec<ChatMessage>,
}

/// The whole chat store: conversations per workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChatConversationsDocument {
    schema_version: u32,
    #[serde(default)]
    workspaces: BTreeMap<Uuid, Vec<ChatConversation>>,
}

impl Default for ChatConversationsDocument {
    fn default() -> Self {
        Self {
            schema_version: DOCUMENT_SCHEMA_VERSION,
            workspaces: BTreeMap::new(),
        }
    }
}

impl VersionedDocument for ChatConversationsDocument {
    type MigrationError = ChatConversationError;

    const FILE_NAME: &'static str = "chat-conversations.json";
    const CURRENT_SCHEMA_VERSION: u32 = DOCUMENT_SCHEMA_VERSION;

    fn migrate(value: Value, version: u32) -> Result<Value, Self::MigrationError> {
        match version {
            DOCUMENT_SCHEMA_VERSION => Ok(value),
            _ => Err(ChatConversationError::Persistence),
        }
    }

    fn validate(&self) -> Result<(), Self::MigrationError> {
        let titles_fit = self
            .workspaces
            .values()
            .flatten()
            .all(|conversation| conversation.title.chars().count() <= MAX_TITLE_CHARS);
        if titles_fit {
            Ok(())
        } else {
            Err(ChatConversationError::Invalid("a stored title is too long"))
        }
    }
}

/// Thread-safe store of chat conversations.
pub(crate) struct ChatConversationService {
    settings: SettingsStore,
    document: Mutex<ChatConversationsDocument>,
    /// False when the saved document could not be read. Nothing is written then, so a failed
    /// read can never overwrite the conversations that are still on disk.
    writable: bool,
}

impl ChatConversationService {
    /// Opens the saved conversations.
    pub(crate) fn new(settings: SettingsStore) -> Result<Self, ChatConversationError> {
        let document = settings
            .load_document::<ChatConversationsDocument>()
            .map_err(|_| ChatConversationError::Persistence)?
            .unwrap_or_default();
        Ok(Self {
            settings,
            document: Mutex::new(document),
            writable: true,
        })
    }

    /// A read-only empty store, used when the saved document could not be read.
    pub(crate) fn unavailable(settings: SettingsStore) -> Self {
        Self {
            settings,
            document: Mutex::new(ChatConversationsDocument::default()),
            writable: false,
        }
    }

    /// Lists a workspace's conversations, most recently changed first.
    pub(crate) fn list(
        &self,
        workspace: Uuid,
    ) -> Result<Vec<ChatConversation>, ChatConversationError> {
        let document = self.lock()?;
        let mut conversations = document
            .workspaces
            .get(&workspace)
            .cloned()
            .unwrap_or_default();
        conversations.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(conversations)
    }

    /// Returns one conversation with its messages.
    pub(crate) fn get(
        &self,
        workspace: Uuid,
        id: Uuid,
    ) -> Result<ChatConversation, ChatConversationError> {
        let document = self.lock()?;
        find(&document, workspace, id).cloned()
    }

    /// Starts an empty conversation in a workspace.
    pub(crate) fn create(
        &self,
        workspace: Uuid,
    ) -> Result<ChatConversation, ChatConversationError> {
        let conversation = ChatConversation {
            id: Uuid::new_v4(),
            title: String::new(),
            updated_at: now(),
            messages: Vec::new(),
        };
        self.mutate(|document| {
            document
                .workspaces
                .entry(workspace)
                .or_default()
                .push(conversation.clone());
            Ok(conversation)
        })
    }

    /// Sets the title. Blank clears it, so the list shows the placeholder again.
    pub(crate) fn rename(
        &self,
        workspace: Uuid,
        id: Uuid,
        title: &str,
    ) -> Result<ChatConversation, ChatConversationError> {
        let title = title.trim();
        if title.chars().count() > MAX_TITLE_CHARS {
            return Err(ChatConversationError::Invalid("the title is too long"));
        }
        self.mutate(|document| {
            let conversation = find_mut(document, workspace, id)?;
            conversation.title = title.to_owned();
            Ok(conversation.clone())
        })
    }

    /// Deletes a conversation and everything saved in it.
    pub(crate) fn delete(&self, workspace: Uuid, id: Uuid) -> Result<(), ChatConversationError> {
        self.mutate(|document| {
            let conversations = document
                .workspaces
                .get_mut(&workspace)
                .ok_or(ChatConversationError::NotFound)?;
            let before = conversations.len();
            conversations.retain(|conversation| conversation.id != id);
            if conversations.len() == before {
                return Err(ChatConversationError::NotFound);
            }
            Ok(())
        })
    }

    /// Appends one message. The first message of an untitled conversation sets its title.
    pub(crate) fn append(
        &self,
        workspace: Uuid,
        id: Uuid,
        message: ChatMessage,
    ) -> Result<ChatConversation, ChatConversationError> {
        self.mutate(|document| {
            let conversation = find_mut(document, workspace, id)?;
            if conversation.title.is_empty() && message.role == ChatRole::User {
                conversation.title = message
                    .content
                    .lines()
                    .next()
                    .unwrap_or_default()
                    .chars()
                    .take(AUTO_TITLE_CHARS)
                    .collect();
            }
            conversation.messages.push(message);
            conversation.updated_at = now();
            Ok(conversation.clone())
        })
    }

    /// Applies one change and saves it. A failed save leaves the in-memory store unchanged.
    fn mutate<T>(
        &self,
        change: impl FnOnce(&mut ChatConversationsDocument) -> Result<T, ChatConversationError>,
    ) -> Result<T, ChatConversationError> {
        if !self.writable {
            return Err(ChatConversationError::Persistence);
        }
        let mut document = self.lock()?;
        let previous = document.clone();
        let outcome = change(&mut document)?;
        if self.settings.save_document(&*document).is_err() {
            *document = previous;
            return Err(ChatConversationError::Persistence);
        }
        Ok(outcome)
    }

    fn lock(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, ChatConversationsDocument>, ChatConversationError> {
        self.document
            .lock()
            .map_err(|_| ChatConversationError::Persistence)
    }
}

fn find(
    document: &ChatConversationsDocument,
    workspace: Uuid,
    id: Uuid,
) -> Result<&ChatConversation, ChatConversationError> {
    document
        .workspaces
        .get(&workspace)
        .and_then(|conversations| {
            conversations
                .iter()
                .find(|conversation| conversation.id == id)
        })
        .ok_or(ChatConversationError::NotFound)
}

fn find_mut(
    document: &mut ChatConversationsDocument,
    workspace: Uuid,
    id: Uuid,
) -> Result<&mut ChatConversation, ChatConversationError> {
    document
        .workspaces
        .get_mut(&workspace)
        .and_then(|conversations| {
            conversations
                .iter_mut()
                .find(|conversation| conversation.id == id)
        })
        .ok_or(ChatConversationError::NotFound)
}

fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// Summary of a conversation for the list, without its messages.
pub(crate) fn summary_to_dto(conversation: &ChatConversation) -> ChatConversationSummaryDto {
    ChatConversationSummaryDto {
        id: conversation.id,
        title: conversation.title.clone(),
        updated_at: conversation.updated_at.clone(),
        message_count: u32::try_from(conversation.messages.len()).unwrap_or(u32::MAX),
    }
}

/// A conversation with its messages, for the tab.
pub(crate) fn conversation_to_dto(conversation: ChatConversation) -> ChatConversationDto {
    ChatConversationDto {
        id: conversation.id,
        title: conversation.title,
        updated_at: conversation.updated_at,
        messages: conversation
            .messages
            .into_iter()
            .map(|message| ChatMessageDto {
                role: match message.role {
                    ChatRole::User => ChatRoleDto::User,
                    ChatRole::Assistant => ChatRoleDto::Assistant,
                },
                content: message.content,
                context: message.context,
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    fn store(directory: &std::path::Path) -> ChatConversationService {
        ChatConversationService::new(SettingsStore::new(directory)).unwrap()
    }

    fn user(content: &str) -> ChatMessage {
        ChatMessage {
            role: ChatRole::User,
            content: content.to_owned(),
            context: None,
        }
    }

    #[test]
    fn conversations_are_listed_per_workspace() {
        let directory = tempdir().unwrap();
        let service = store(directory.path());
        let (first, second) = (Uuid::new_v4(), Uuid::new_v4());
        let a = service.create(first).unwrap();
        service.create(second).unwrap();
        assert_eq!(service.list(first).unwrap().len(), 1);
        assert_eq!(service.list(first).unwrap()[0].id, a.id);
        assert!(service.get(second, a.id).is_err());
    }

    #[test]
    fn first_message_titles_an_untitled_conversation_and_renames_win() {
        let directory = tempdir().unwrap();
        let service = store(directory.path());
        let workspace = Uuid::new_v4();
        let id = service.create(workspace).unwrap().id;
        let long = format!("{}\nsecond line", "x".repeat(80));
        let titled = service.append(workspace, id, user(&long)).unwrap();
        assert_eq!(titled.title, "x".repeat(60));
        let renamed = service.rename(workspace, id, "  Notes  ").unwrap();
        assert_eq!(renamed.title, "Notes");
        let kept = service
            .append(workspace, id, user("later question"))
            .unwrap();
        assert_eq!(kept.title, "Notes");
    }

    #[test]
    fn rename_rejects_long_titles_and_blank_clears() {
        let directory = tempdir().unwrap();
        let service = store(directory.path());
        let workspace = Uuid::new_v4();
        let id = service.create(workspace).unwrap().id;
        assert_eq!(
            service.rename(workspace, id, &"t".repeat(MAX_TITLE_CHARS + 1)),
            Err(ChatConversationError::Invalid("the title is too long"))
        );
        service.rename(workspace, id, "Plan").unwrap();
        assert_eq!(service.rename(workspace, id, "   ").unwrap().title, "");
    }

    #[test]
    fn delete_removes_only_that_conversation() {
        let directory = tempdir().unwrap();
        let service = store(directory.path());
        let workspace = Uuid::new_v4();
        let keep = service.create(workspace).unwrap().id;
        let gone = service.create(workspace).unwrap().id;
        service.delete(workspace, gone).unwrap();
        assert_eq!(
            service.delete(workspace, gone),
            Err(ChatConversationError::NotFound)
        );
        let remaining: Vec<Uuid> = service
            .list(workspace)
            .unwrap()
            .iter()
            .map(|conversation| conversation.id)
            .collect();
        assert_eq!(remaining, vec![keep]);
    }

    #[test]
    fn conversations_and_context_survive_a_restart() {
        let directory = tempdir().unwrap();
        let workspace = Uuid::new_v4();
        let id = {
            let service = store(directory.path());
            let id = service.create(workspace).unwrap().id;
            service
                .append(
                    workspace,
                    id,
                    ChatMessage {
                        role: ChatRole::User,
                        content: "What is large here?".to_owned(),
                        context: Some("Folder listing:\n- big.iso (4 GB)".to_owned()),
                    },
                )
                .unwrap();
            id
        };
        let reopened = store(directory.path());
        let saved = reopened.get(workspace, id).unwrap();
        assert_eq!(saved.messages.len(), 1);
        assert_eq!(
            saved.messages[0].context.as_deref(),
            Some("Folder listing:\n- big.iso (4 GB)")
        );
    }

    #[test]
    fn an_unreadable_store_refuses_writes() {
        let directory = tempdir().unwrap();
        let workspace = Uuid::new_v4();
        // A file where the settings directory should be: the document can be neither read nor saved.
        let blocker = directory.path().join("blocked");
        std::fs::write(&blocker, b"not a directory").unwrap();
        let blocked =
            ChatConversationService::unavailable(SettingsStore::new(blocker.join("inner")));
        assert_eq!(
            blocked.create(workspace),
            Err(ChatConversationError::Persistence)
        );
        assert!(blocked.list(workspace).unwrap().is_empty());
    }

    #[test]
    fn a_failed_save_leaves_the_store_unchanged() {
        let directory = tempdir().unwrap();
        let service = store(directory.path());
        let workspace = Uuid::new_v4();
        let kept = service.create(workspace).unwrap().id;
        let original = std::fs::metadata(directory.path()).unwrap().permissions();
        let mut readonly = original.clone();
        readonly.set_readonly(true);
        std::fs::set_permissions(directory.path(), readonly).unwrap();
        let outcome = service.create(workspace);
        std::fs::set_permissions(directory.path(), original).unwrap();
        assert_eq!(outcome, Err(ChatConversationError::Persistence));
        let ids: Vec<Uuid> = service
            .list(workspace)
            .unwrap()
            .iter()
            .map(|conversation| conversation.id)
            .collect();
        assert_eq!(ids, vec![kept]);
    }

    #[test]
    fn summaries_count_messages_without_exposing_them() {
        let directory = tempdir().unwrap();
        let service = store(directory.path());
        let workspace = Uuid::new_v4();
        let id = service.create(workspace).unwrap().id;
        let conversation = service.append(workspace, id, user("hello")).unwrap();
        let summary = summary_to_dto(&conversation);
        assert_eq!(summary.message_count, 1);
        assert_eq!(summary.title, "hello");
    }
}
