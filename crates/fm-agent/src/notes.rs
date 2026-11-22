//! The assistant's notes tools (task 0337), behind an interface the notes capability implements.
//! Searching and reading run without approval and stay inside the notes folder, also for an SSH
//! terminal. A proposed note ends the turn: the person reviews it and nothing is written before
//! they save it.

use async_trait::async_trait;
use fm_transport_dto::NoteProposalDto;

use crate::skills::NoteSkill;

/// One notes tool the model asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoteToolCall {
    /// Searches the notes.
    Search {
        /// What to look for.
        query: String,
        /// Most hits wanted.
        limit: Option<u32>,
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

/// What a notes tool produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoteToolOutcome {
    /// A line for the person and the text for the model.
    Output {
        /// A line for the person.
        step: String,
        /// The text for the model.
        text: String,
    },
    /// A note waiting for the person's review.
    Proposal(NoteProposalDto),
}

/// The notes folder as the assistant may use it.
#[async_trait]
pub trait NotesAccess: Send + Sync {
    /// Runs one notes tool.
    async fn run(&self, call: NoteToolCall) -> NoteToolOutcome;

    /// The notes that best match `query`, as skills (task 0339). None without notes.
    async fn skills(&self, _query: &str) -> Vec<NoteSkill> {
        Vec::new()
    }
}
