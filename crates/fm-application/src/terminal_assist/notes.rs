//! The assistant's notes tools (task 0337), behind an interface the notes capability implements.
//! Searching and reading run without approval and stay inside the notes folder, also for an SSH
//! terminal. A proposed note ends the turn: the person reviews it and nothing is written before
//! they save it.

use async_trait::async_trait;
use fm_transport_dto::NoteProposalDto;

/// One notes tool the model asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NoteToolCall {
    Search {
        query: String,
        limit: Option<u32>,
    },
    Read {
        path: String,
    },
    /// A new note (`path` is `None`) or a replacement for an existing one.
    Propose {
        path: Option<String>,
        title: Option<String>,
        content: String,
    },
}

/// What a notes tool produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NoteToolOutcome {
    /// A line for the person and the text for the model.
    Output { step: String, text: String },
    /// A note waiting for the person's review.
    Proposal(NoteProposalDto),
}

/// The notes folder as the assistant may use it.
#[async_trait]
pub(crate) trait NotesAccess: Send + Sync {
    async fn run(&self, call: NoteToolCall) -> NoteToolOutcome;
}
