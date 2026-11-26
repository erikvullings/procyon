//! The notes tools, behind an interface the host's notes store implements. Searching and reading
//! run without approval and stay inside the notes folder. A proposed note stops the turn: the
//! person reviews it, and nothing is written before they save it.

use async_trait::async_trait;

use crate::skills::NoteSkill;
use crate::tool::ToolOutcome;

/// One notes tool the model asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoteToolCall {
    /// Searches the notes.
    Search {
        /// What to look for.
        query: String,
        /// Most hits wanted.
        limit: Option<u32>,
        /// Notes only, past assistant sessions only, or both (`None`).
        kind: Option<NoteKind>,
    },
    /// Reads one note.
    Read {
        /// The note's path in the notes folder.
        path: String,
    },
    /// A new note (`path` is `None`) or a replacement for an existing one.
    Propose {
        /// The note to replace, or `None` for a new note.
        path: Option<String>,
        /// The title for a new note.
        title: Option<String>,
        /// The proposed Markdown.
        content: String,
    },
}

/// What a hit in the notes folder is (task 0358).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum NoteKind {
    /// A note the person wrote or saved.
    Note,
    /// The record of an earlier assistant session (its summary, questions and files).
    Session,
}

/// A note waiting for the person's review.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteProposal {
    /// The existing note to replace, inside the notes folder; `None` for a new note.
    pub path: Option<String>,
    /// The note's title.
    pub title: String,
    /// Suggested file name inside the notes folder.
    pub file_name: String,
    /// Front-matter tags.
    pub tags: Vec<String>,
    /// The complete Markdown file, front matter included.
    pub content: String,
    /// How many values were replaced by a redaction mark; the person should check them.
    pub redactions: u32,
}

/// The notes folder as the agent may use it.
#[async_trait]
pub trait NotesAccess: Send + Sync {
    /// Runs one notes tool. A proposal is the interrupt.
    async fn run(&self, call: NoteToolCall) -> ToolOutcome<NoteProposal>;

    /// The notes that best match `query`, offered as skills. None by default.
    async fn skills(&self, _query: &str) -> Vec<NoteSkill> {
        Vec::new()
    }

    /// Up to `limit` notes offered as skills whatever the request, for a host that lets the person
    /// pick one by name (`/name`). None by default.
    async fn list_skills(&self, _limit: usize) -> Vec<NoteSkill> {
        Vec::new()
    }
}
