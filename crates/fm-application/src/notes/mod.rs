//! Notes (task 0337): plain Markdown files in one folder the person chooses, so Obsidian or any
//! editor can open the same folder. `NotesService` owns the notes folder setting, turns terminal
//! assistant sessions into draft notes, searches notes (semantically when the folder is indexed,
//! by text otherwise) and reads them inside the folder only. It never writes a note: new notes
//! are created through the operation engine and changes saved through the editor, both after the
//! person reviews them.

mod capture;
mod format;
mod search;
mod semantic;
mod store;
mod tools;

use std::sync::{Arc, Mutex};

use chrono::Utc;
use fm_domain::Location;
use fm_settings::{Settings, SettingsStore};
use fm_transport_dto::{
    ChatFailureDto, DraftNoteRequestDto, NoteDraftDto, NoteHitDto, NotesSearchResultDto,
    NotesSearchStrategyDto, NotesSetupDto, SearchNotesRequestDto,
};
use fm_vfs::ProviderRegistry;
use tokio_util::sync::CancellationToken;

use crate::chat::ChatGenerator;
use crate::error::ApplicationError;
use crate::rejection::{Rejection, reason};

use self::capture::{CaptureInput, draft_note};
pub(crate) use self::search::SemanticNotes;
use self::search::{DEFAULT_HITS, SearchStrategy, search_notes};
pub(crate) use self::semantic::KnowledgeNotesIndex;
pub(crate) use self::store::{NotesError, NotesStore};
pub(crate) use self::tools::AssistantNotes;

/// Name of the suggested notes folder.
const SUGGESTED_FOLDER_NAME: &str = "Procyon Notes";
/// Longest search query, in characters.
const MAX_QUERY_CHARS: usize = 500;

/// The notes capability.
pub(crate) struct NotesService {
    providers: ProviderRegistry,
    settings: Arc<Mutex<Settings>>,
    settings_store: SettingsStore,
    suggested_root: Option<Location>,
}

impl NotesService {
    /// `suggested_root` is what setup offers; a server host passes `None` and guesses nothing.
    pub(crate) fn new(
        providers: ProviderRegistry,
        settings: Arc<Mutex<Settings>>,
        settings_store: SettingsStore,
        suggested_root: Option<Location>,
    ) -> Self {
        Self {
            providers,
            settings,
            settings_store,
            suggested_root,
        }
    }

    /// The desktop suggestion: `Procyon Notes` in the documents folder, else in the home folder.
    pub(crate) fn desktop_suggested_root() -> Option<Location> {
        let parent = dirs::document_dir().or_else(dirs::home_dir)?;
        Location::from_native_path(&parent.join(SUGGESTED_FOLDER_NAME)).ok()
    }

    /// The configured notes folder.
    pub(crate) fn root(&self) -> Option<Location> {
        self.settings
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .notes_root
            .clone()
    }

    /// Where notes live, whether that folder exists, and what setup suggests.
    pub(crate) async fn setup(&self) -> NotesSetupDto {
        let root = self.root();
        let root_exists = match &root {
            Some(root) => NotesStore::open(&self.providers, root).await.is_ok(),
            None => false,
        };
        NotesSetupDto {
            root: root.map(Into::into),
            root_exists,
            suggested_root: self.suggested_root.clone().map(Into::into),
        }
    }

    /// Stores an existing folder as the notes folder. Files are never moved.
    pub(crate) async fn set_root(&self, location: Location) -> Result<NotesSetupDto, NotesError> {
        NotesStore::open(&self.providers, &location).await?;
        {
            let mut settings = self
                .settings
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let mut changed = settings.clone();
            changed.notes_root = Some(location);
            self.settings_store
                .save(&changed)
                .map_err(|_| NotesError::RootUnavailable)?;
            *settings = changed;
        }
        Ok(self.setup().await)
    }

    /// The notes folder, opened.
    pub(crate) async fn store(&self) -> Result<NotesStore, NotesError> {
        let root = self.root().ok_or(NotesError::NotConfigured)?;
        NotesStore::open(&self.providers, &root).await
    }

    /// Searches notes; `semantic` is the folder's index when the caller may use it.
    pub(crate) async fn search(
        &self,
        store: &NotesStore,
        semantic: Option<&dyn SemanticNotes>,
        request: &SearchNotesRequestDto,
    ) -> NotesSearchResultDto {
        let query: String = request.query.trim().chars().take(MAX_QUERY_CHARS).collect();
        let limit = request.limit.map_or(DEFAULT_HITS, |limit| {
            usize::try_from(limit).unwrap_or(DEFAULT_HITS)
        });
        let (strategy, hits) =
            search_notes(store, semantic, &query, limit, &CancellationToken::new()).await;
        NotesSearchResultDto {
            strategy: match strategy {
                SearchStrategy::Semantic => NotesSearchStrategyDto::Semantic,
                SearchStrategy::Text => NotesSearchStrategyDto::Text,
            },
            hits: hits
                .into_iter()
                .map(|hit| NoteHitDto {
                    location: hit.location.into(),
                    path: hit.path,
                    title: hit.title,
                    tags: hit.tags,
                    snippet: hit.snippet,
                    score: hit.score,
                })
                .collect(),
        }
    }

    /// Drafts a note from a terminal assistant session for the person to review.
    pub(crate) async fn draft(
        &self,
        generator: &impl ChatGenerator,
        conversation: String,
        request: DraftNoteRequestDto,
    ) -> Result<NoteDraftDto, ChatFailureDto> {
        draft_note(
            generator,
            CaptureInput {
                conversation,
                blocks: request.blocks,
                working_directory: request.working_directory,
                shell: request.shell,
                host: request.host,
                title_hint: request.title_hint,
                now: Utc::now(),
            },
        )
        .await
    }
}

/// A notes failure as a rejection the client localises.
pub(crate) fn notes_error(error: NotesError) -> ApplicationError {
    let rejection = match &error {
        NotesError::NotConfigured => {
            Rejection::new(reason::NOTES_NOT_CONFIGURED, error.to_string())
        }
        NotesError::RootUnavailable => {
            Rejection::new(reason::NOTES_ROOT_UNAVAILABLE, error.to_string())
        }
        NotesError::NotAvailable(path) => {
            Rejection::new(reason::NOTES_PATH_UNAVAILABLE, error.to_string()).with("path", path)
        }
        NotesError::NotText(path) => {
            Rejection::new(reason::NOTES_NOT_TEXT, error.to_string()).with("path", path)
        }
    };
    ApplicationError::from(rejection)
}

#[cfg(test)]
mod tests;
