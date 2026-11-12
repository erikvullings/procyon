//! Saved chat conversations (task 0324). Each conversation is one versioned document in its own
//! directory, `chat-conversations/<workspace>/<conversation>/conversation.json` under the settings
//! data root. A change rewrites only that conversation's file. Conversations never leave the
//! machine except in a reply request, which [`crate::chat`] sends.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
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

/// Schema of one conversation document written by this build.
const DOCUMENT_SCHEMA_VERSION: u32 = 1;
/// Directory under the settings data root that holds every workspace's conversations.
const STORE_DIRECTORY: &str = "chat-conversations";
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

/// The versioned file that holds one conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConversationDocument {
    schema_version: u32,
    #[serde(flatten)]
    conversation: ChatConversation,
}

impl VersionedDocument for ConversationDocument {
    type MigrationError = ChatConversationError;

    const FILE_NAME: &'static str = "conversation.json";
    const CURRENT_SCHEMA_VERSION: u32 = DOCUMENT_SCHEMA_VERSION;

    fn migrate(value: Value, version: u32) -> Result<Value, Self::MigrationError> {
        match version {
            DOCUMENT_SCHEMA_VERSION => Ok(value),
            _ => Err(ChatConversationError::Persistence),
        }
    }

    fn validate(&self) -> Result<(), Self::MigrationError> {
        if self.conversation.title.chars().count() <= MAX_TITLE_CHARS {
            Ok(())
        } else {
            Err(ChatConversationError::Invalid("a stored title is too long"))
        }
    }
}

/// Conversations by workspace, then by id.
type Conversations = BTreeMap<Uuid, BTreeMap<Uuid, ChatConversation>>;

/// Thread-safe store of chat conversations. Every loaded conversation is held in memory; a change
/// is saved to that conversation's own file before the memory copy is replaced.
pub(crate) struct ChatConversationService {
    root: PathBuf,
    conversations: Mutex<Conversations>,
    /// False when the store directory could not be read. Nothing is written then, so a failed read
    /// can never lead to a save over conversations that are still on disk.
    writable: bool,
}

impl ChatConversationService {
    /// Loads every readable conversation under the settings data root.
    pub(crate) fn new(settings: &SettingsStore) -> Result<Self, ChatConversationError> {
        let root = settings.directory().join(STORE_DIRECTORY);
        let conversations = load_all(&root)?;
        Ok(Self {
            root,
            conversations: Mutex::new(conversations),
            writable: true,
        })
    }

    /// A read-only empty store, used when the store directory could not be read.
    pub(crate) fn unavailable(settings: &SettingsStore) -> Self {
        Self {
            root: settings.directory().join(STORE_DIRECTORY),
            conversations: Mutex::new(BTreeMap::new()),
            writable: false,
        }
    }

    /// Lists a workspace's conversations, most recently changed first.
    pub(crate) fn list(
        &self,
        workspace: Uuid,
    ) -> Result<Vec<ChatConversation>, ChatConversationError> {
        let all = self.lock()?;
        let mut conversations: Vec<ChatConversation> = all
            .get(&workspace)
            .map(|conversations| conversations.values().cloned().collect())
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
        let all = self.lock()?;
        all.get(&workspace)
            .and_then(|conversations| conversations.get(&id))
            .cloned()
            .ok_or(ChatConversationError::NotFound)
    }

    /// Starts an empty conversation in a workspace.
    pub(crate) fn create(
        &self,
        workspace: Uuid,
    ) -> Result<ChatConversation, ChatConversationError> {
        self.require_writable()?;
        let conversation = ChatConversation {
            id: Uuid::new_v4(),
            title: String::new(),
            updated_at: now(),
            messages: Vec::new(),
        };
        let mut all = self.lock()?;
        save(&self.root, workspace, &conversation)?;
        all.entry(workspace)
            .or_default()
            .insert(conversation.id, conversation.clone());
        Ok(conversation)
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
        self.update(workspace, id, |conversation| {
            conversation.title = title.to_owned();
            Ok(conversation.clone())
        })
    }

    /// Deletes a conversation and everything saved in it.
    pub(crate) fn delete(&self, workspace: Uuid, id: Uuid) -> Result<(), ChatConversationError> {
        self.require_writable()?;
        let mut all = self.lock()?;
        if !all
            .get(&workspace)
            .is_some_and(|conversations| conversations.contains_key(&id))
        {
            return Err(ChatConversationError::NotFound);
        }
        match fs::remove_dir_all(conversation_dir(&self.root, workspace, id)) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(ChatConversationError::Persistence),
        }
        if let Some(conversations) = all.get_mut(&workspace) {
            conversations.remove(&id);
        }
        Ok(())
    }

    /// Appends one message. The first message of an untitled conversation sets its title.
    pub(crate) fn append(
        &self,
        workspace: Uuid,
        id: Uuid,
        message: ChatMessage,
    ) -> Result<ChatConversation, ChatConversationError> {
        self.update(workspace, id, |conversation| {
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

    /// Applies a change to one conversation and saves that conversation only. A failed save leaves
    /// the in-memory copy unchanged.
    fn update<T>(
        &self,
        workspace: Uuid,
        id: Uuid,
        change: impl FnOnce(&mut ChatConversation) -> Result<T, ChatConversationError>,
    ) -> Result<T, ChatConversationError> {
        self.require_writable()?;
        let mut all = self.lock()?;
        let stored = all
            .get(&workspace)
            .and_then(|conversations| conversations.get(&id))
            .ok_or(ChatConversationError::NotFound)?;
        let mut next = stored.clone();
        let outcome = change(&mut next)?;
        save(&self.root, workspace, &next)?;
        all.entry(workspace).or_default().insert(id, next);
        Ok(outcome)
    }

    fn require_writable(&self) -> Result<(), ChatConversationError> {
        if self.writable {
            Ok(())
        } else {
            Err(ChatConversationError::Persistence)
        }
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Conversations>, ChatConversationError> {
        self.conversations
            .lock()
            .map_err(|_| ChatConversationError::Persistence)
    }
}

fn conversation_dir(root: &Path, workspace: Uuid, id: Uuid) -> PathBuf {
    root.join(workspace.to_string()).join(id.to_string())
}

/// Saves one conversation to its own file, creating its directory first.
fn save(
    root: &Path,
    workspace: Uuid,
    conversation: &ChatConversation,
) -> Result<(), ChatConversationError> {
    let directory = conversation_dir(root, workspace, conversation.id);
    fs::create_dir_all(&directory).map_err(|_| ChatConversationError::Persistence)?;
    SettingsStore::new(&directory)
        .save_document(&ConversationDocument {
            schema_version: DOCUMENT_SCHEMA_VERSION,
            conversation: conversation.clone(),
        })
        .map_err(|_| ChatConversationError::Persistence)
}

/// Reads every conversation under `root`. A conversation whose file cannot be read is left out of
/// the list and is never written, because writes only touch conversations loaded here.
fn load_all(root: &Path) -> Result<Conversations, ChatConversationError> {
    let mut all = Conversations::new();
    let workspaces = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(all),
        Err(_) => return Err(ChatConversationError::Persistence),
    };
    for workspace in workspaces.flatten() {
        let Some(workspace_id) = parse_id(&workspace.file_name()) else {
            continue;
        };
        let Ok(conversation_dirs) = fs::read_dir(workspace.path()) else {
            continue;
        };
        for conversation_dir in conversation_dirs.flatten() {
            let Some(id) = parse_id(&conversation_dir.file_name()) else {
                continue;
            };
            let loaded =
                SettingsStore::new(conversation_dir.path()).load_document::<ConversationDocument>();
            if let Ok(Some(document)) = loaded
                && document.conversation.id == id
            {
                all.entry(workspace_id)
                    .or_default()
                    .insert(id, document.conversation);
            }
        }
    }
    Ok(all)
}

fn parse_id(name: &OsStr) -> Option<Uuid> {
    name.to_str().and_then(|text| Uuid::parse_str(text).ok())
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

    fn store(settings_directory: &Path) -> ChatConversationService {
        ChatConversationService::new(&SettingsStore::new(settings_directory)).unwrap()
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
    fn each_conversation_is_saved_in_its_own_file() {
        let directory = tempdir().unwrap();
        let service = store(directory.path());
        let workspace = Uuid::new_v4();
        let first = service.create(workspace).unwrap().id;
        let second = service.create(workspace).unwrap().id;
        service.append(workspace, first, user("hello")).unwrap();
        let root = directory.path().join(STORE_DIRECTORY);
        assert!(
            conversation_dir(&root, workspace, first)
                .join("conversation.json")
                .is_file()
        );
        assert!(
            conversation_dir(&root, workspace, second)
                .join("conversation.json")
                .is_file()
        );
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
    fn delete_removes_only_that_conversation_and_its_file() {
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
        let root = directory.path().join(STORE_DIRECTORY);
        assert!(!conversation_dir(&root, workspace, gone).exists());
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
        // A file where the settings directory should be: the store can be neither read nor saved.
        let blocker = directory.path().join("blocked");
        std::fs::write(&blocker, b"not a directory").unwrap();
        let blocked =
            ChatConversationService::unavailable(&SettingsStore::new(blocker.join("inner")));
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
        // The workspace directory is read-only, so a new conversation cannot be written into it.
        let workspace_dir = directory
            .path()
            .join(STORE_DIRECTORY)
            .join(workspace.to_string());
        let original = std::fs::metadata(&workspace_dir).unwrap().permissions();
        let mut readonly = original.clone();
        readonly.set_readonly(true);
        std::fs::set_permissions(&workspace_dir, readonly).unwrap();
        let outcome = service.create(workspace);
        std::fs::set_permissions(&workspace_dir, original).unwrap();
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
    fn an_unreadable_conversation_is_left_out_and_never_overwritten() {
        let directory = tempdir().unwrap();
        let workspace = Uuid::new_v4();
        let damaged = Uuid::new_v4();
        let damaged_dir = directory
            .path()
            .join(STORE_DIRECTORY)
            .join(workspace.to_string())
            .join(damaged.to_string());
        std::fs::create_dir_all(&damaged_dir).unwrap();
        std::fs::write(damaged_dir.join("conversation.json"), b"{ not json").unwrap();

        let service = store(directory.path());
        let created = service.create(workspace).unwrap().id;

        let ids: Vec<Uuid> = service
            .list(workspace)
            .unwrap()
            .iter()
            .map(|conversation| conversation.id)
            .collect();
        assert_eq!(ids, vec![created]);
        assert_eq!(
            std::fs::read(damaged_dir.join("conversation.json")).unwrap(),
            b"{ not json"
        );
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
