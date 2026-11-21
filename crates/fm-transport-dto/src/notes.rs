//! Transport types for notes (task 0337): plain Markdown files in one folder the person chooses,
//! searched semantically when that folder is indexed and by text otherwise. Notes are written
//! through the operation engine and the editor, never by these requests.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{LocationDto, TerminalBlockDto};

/// Where notes live and what setup would suggest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NotesSetupDto {
    /// The configured notes folder; `None` until notes are set up.
    pub root: Option<LocationDto>,
    /// Whether the configured folder currently exists as a folder.
    pub root_exists: bool,
    /// The folder setup suggests on this host; `None` when the host guesses none (a server).
    pub suggested_root: Option<LocationDto>,
}

/// Checks or stores the notes folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetNotesRootRequestDto {
    /// The folder. It must exist; setup creates it first through the operation engine.
    pub location: LocationDto,
}

/// Which search produced the hits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum NotesSearchStrategyDto {
    /// The semantic index of the notes folder.
    Semantic,
    /// A bounded text search over the notes folder.
    Text,
}

/// Searches the notes folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SearchNotesRequestDto {
    /// Workspace whose semantic consent applies; without one only the text search runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<Uuid>,
    /// What to look for.
    pub query: String,
    /// Most hits returned; the host caps it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// One matching note.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NoteHitDto {
    /// The note file.
    pub location: LocationDto,
    /// Its path inside the notes folder, with `/` separators.
    pub path: String,
    /// The front-matter title, the first heading, or the file name.
    pub title: String,
    /// Front-matter tags.
    pub tags: Vec<String>,
    /// The best matching line or passage, bounded.
    pub snippet: String,
    /// Relative rank within this result; higher is better.
    pub score: f32,
}

/// Ranked notes and how they were found.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NotesSearchResultDto {
    /// The search that produced `hits`.
    pub strategy: NotesSearchStrategyDto,
    /// Best first.
    pub hits: Vec<NoteHitDto>,
}

/// Reads one note inside the notes folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReadNoteRequestDto {
    /// Path inside the notes folder, with `/` separators.
    pub path: String,
}

/// One note's text, with the revision the editor save expects.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NoteDto {
    /// The note file.
    pub location: LocationDto,
    /// Its path inside the notes folder.
    pub path: String,
    /// The whole file.
    pub content: String,
    /// Revision for `saveEditableFile`.
    pub revision: String,
}

/// Turns a terminal assistant session into a draft note (desktop host only, like the terminal).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftNoteRequestDto {
    /// The terminal tab's assistant session, whose conversation is included when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// Finished commands to include, oldest first.
    #[serde(default)]
    pub blocks: Vec<TerminalBlockDto>,
    /// Where the shell was working, as the location URI.
    pub working_directory: String,
    /// The shell's name when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shell: Option<String>,
    /// The machine the shell ran on, for example an SSH host name; `None` for this computer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    /// A title the person already typed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title_hint: Option<String>,
}

/// A note proposed for review. Nothing is written until the person saves it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteDraftDto {
    /// The note's title.
    pub title: String,
    /// Suggested file name inside the notes folder, ending in `.md`.
    pub file_name: String,
    /// Front-matter tags.
    pub tags: Vec<String>,
    /// The complete Markdown file, front matter included.
    pub content: String,
    /// How many values were replaced by a `[redacted…]` mark; the person should check them.
    pub redactions: u32,
}

/// A note the terminal assistant proposes to create or change (task 0337).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteProposalDto {
    /// The existing note to replace, inside the notes folder; `None` for a new note.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// The proposed note, as a draft.
    pub draft: NoteDraftDto,
}
