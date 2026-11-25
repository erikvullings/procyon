//! The built-in tools: looking at files, the notes, and asking the person to run a command. File
//! and notes access come through the [`ToolContext`], so each host decides what a path means and
//! what is allowed.

use async_trait::async_trait;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::notes::{NoteProposal, NoteToolCall};
use crate::tool::{ToolContext, ToolEffect, ToolOutcome, TypedTool};

fn unavailable<I>(step: &str, what: &str) -> ToolOutcome<I> {
    ToolOutcome::error(step, format!("{what} are not available here."))
}

fn blank<I>(step: &str, what: &str) -> ToolOutcome<I> {
    ToolOutcome::error(step, format!("{what} must not be empty."))
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ListDirArgs {
    /// Folder relative to the working folder; `.` for the working folder itself.
    path: String,
}

pub(crate) struct ListDir;

#[async_trait]
impl<I: Send + 'static> TypedTool<I> for ListDir {
    type Args = ListDirArgs;

    fn argument_aliases(&self) -> &'static [(&'static str, &'static [&'static str])] {
        &[("path", &["file_path", "filename"])]
    }

    fn name(&self) -> &'static str {
        "list_dir"
    }

    fn description(&self) -> &'static str {
        "Lists a folder. Paths are relative to the working folder and cannot leave it."
    }

    fn effect(&self) -> ToolEffect {
        ToolEffect::ReadOnly
    }

    async fn run_typed(&self, ctx: &ToolContext<'_>, args: ListDirArgs) -> ToolOutcome<I> {
        const STEP: &str = "Listed a folder";
        if args.path.trim().is_empty() {
            return blank(STEP, "path");
        }
        match ctx.files {
            Some(files) => files.list_dir(&args.path).await.into(),
            None => unavailable(STEP, "file tools"),
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ReadFileArgs {
    /// File relative to the working folder.
    path: String,
    /// Byte offset to continue a file that an earlier read cut off.
    offset: Option<usize>,
}

pub(crate) struct ReadFile;

#[async_trait]
impl<I: Send + 'static> TypedTool<I> for ReadFile {
    type Args = ReadFileArgs;

    fn argument_aliases(&self) -> &'static [(&'static str, &'static [&'static str])] {
        &[("path", &["file_path", "filename"])]
    }

    fn name(&self) -> &'static str {
        "read_file"
    }

    fn description(&self) -> &'static str {
        "Reads a text file, or the text of a document the host can convert. A long file is cut \
         off with a note: continue with the offset it names."
    }

    fn effect(&self) -> ToolEffect {
        ToolEffect::ReadOnly
    }

    async fn run_typed(&self, ctx: &ToolContext<'_>, args: ReadFileArgs) -> ToolOutcome<I> {
        const STEP: &str = "Read a file";
        if args.path.trim().is_empty() {
            return blank(STEP, "path");
        }
        match ctx.files {
            Some(files) => files
                .read_file(&args.path, args.offset.unwrap_or(0))
                .await
                .into(),
            None => unavailable(STEP, "file tools"),
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct GrepArgs {
    /// The text to find (not a regular expression).
    pattern: String,
    /// Folder or file to search, relative to the working folder; the working folder when left out.
    path: Option<String>,
}

pub(crate) struct Grep;

#[async_trait]
impl<I: Send + 'static> TypedTool<I> for Grep {
    type Args = GrepArgs;

    fn argument_aliases(&self) -> &'static [(&'static str, &'static [&'static str])] {
        &[("path", &["file_path", "filename"])]
    }

    fn name(&self) -> &'static str {
        "grep"
    }

    fn description(&self) -> &'static str {
        "Finds lines containing text, in a folder or a file."
    }

    fn effect(&self) -> ToolEffect {
        ToolEffect::ReadOnly
    }

    async fn run_typed(&self, ctx: &ToolContext<'_>, args: GrepArgs) -> ToolOutcome<I> {
        const STEP: &str = "Searched files";
        if args.pattern.is_empty() {
            return blank(STEP, "pattern");
        }
        match ctx.files {
            Some(files) => files.grep(&args.pattern, args.path.as_deref()).await.into(),
            None => unavailable(STEP, "file tools"),
        }
    }
}

/// A command the model wants the person to run: the interrupt of `run_command`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandRequest {
    /// The one line to run, trimmed.
    pub command: String,
    /// What the command does and why, in the person's terms (task 0364).
    pub explanation: String,
}

/// Longest explanation kept, in characters.
const MAX_EXPLANATION_CHARS: usize = 600;

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct RunCommandArgs {
    /// One shell command line.
    command: String,
    /// One or two sentences for the person: what the command does and why, naming what it
    /// reads, changes, deletes or downloads.
    explanation: String,
}

/// Asks the person to run one command; the host does the asking.
pub(crate) struct RunCommand;

#[async_trait]
impl<I: From<CommandRequest> + Send + 'static> TypedTool<I> for RunCommand {
    type Args = RunCommandArgs;

    fn argument_aliases(&self) -> &'static [(&'static str, &'static [&'static str])] {
        &[("command", &["cmd"])]
    }

    fn name(&self) -> &'static str {
        "run_command"
    }

    fn description(&self) -> &'static str {
        "Asks the person to run a command in their terminal. They may decline, and the result \
         returns with its exit status and output. Use it only for what your other tools cannot \
         do: never to list, find, read, convert, summarise or create files, and never write a \
         script (python, node, shell) for such work; compare file names yourself. Always give \
         `explanation`: one or two plain sentences on what the command does and why, naming \
         what it reads, changes, deletes or downloads."
    }

    async fn run_typed(&self, _ctx: &ToolContext<'_>, args: RunCommandArgs) -> ToolOutcome<I> {
        // One approval must run exactly one line: a newline or control byte would run more.
        if args.command.trim().is_empty() || args.command.chars().any(char::is_control) {
            return ToolOutcome::error(
                "Run command",
                "the command must be one non-empty line without control characters.",
            );
        }
        let explanation = args.explanation.trim();
        if explanation.is_empty() {
            return ToolOutcome::error(
                "Run command",
                "give `explanation`: one or two sentences on what the command does and why, \
                 naming what it reads, changes, deletes or downloads. The person sees it before \
                 deciding.",
            );
        }
        ToolOutcome::Interrupt(I::from(CommandRequest {
            command: args.command.trim().to_owned(),
            explanation: explanation.chars().take(MAX_EXPLANATION_CHARS).collect(),
        }))
    }
}

/// Runs a notes call through the notes capability, or explains that there is none.
async fn notes<I: From<NoteProposal>>(
    ctx: &ToolContext<'_>,
    step: &str,
    call: NoteToolCall,
) -> ToolOutcome<I> {
    match ctx.notes {
        Some(notes) => notes.run(call).await.map_interrupt(I::from),
        None => unavailable(step, "notes"),
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct SearchNotesArgs {
    /// What to look for, in a few words.
    query: String,
    /// Most notes wanted.
    limit: Option<u32>,
}

pub(crate) struct SearchNotes;

#[async_trait]
impl<I: From<NoteProposal> + Send + 'static> TypedTool<I> for SearchNotes {
    type Args = SearchNotesArgs;

    fn name(&self) -> &'static str {
        "search_notes"
    }

    fn description(&self) -> &'static str {
        "Searches the person's notes (runbooks for tasks they do rarely). Search them before \
         answering a how-to question."
    }

    fn effect(&self) -> ToolEffect {
        ToolEffect::ReadOnly
    }

    async fn run_typed(&self, ctx: &ToolContext<'_>, args: SearchNotesArgs) -> ToolOutcome<I> {
        const STEP: &str = "Looked in the notes";
        if args.query.trim().is_empty() {
            return blank(STEP, "query");
        }
        let call = NoteToolCall::Search {
            query: args.query,
            limit: args.limit,
        };
        notes(ctx, STEP, call).await
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ReadNoteArgs {
    /// The note's path, as search_notes gave it.
    path: String,
}

pub(crate) struct ReadNote;

#[async_trait]
impl<I: From<NoteProposal> + Send + 'static> TypedTool<I> for ReadNote {
    type Args = ReadNoteArgs;

    fn name(&self) -> &'static str {
        "read_note"
    }

    fn description(&self) -> &'static str {
        "Reads one note by the path search_notes gave."
    }

    fn effect(&self) -> ToolEffect {
        ToolEffect::ReadOnly
    }

    async fn run_typed(&self, ctx: &ToolContext<'_>, args: ReadNoteArgs) -> ToolOutcome<I> {
        const STEP: &str = "Read a note";
        if args.path.trim().is_empty() {
            return blank(STEP, "path");
        }
        notes(ctx, STEP, NoteToolCall::Read { path: args.path }).await
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ProposeNoteArgs {
    /// The note's title.
    title: Option<String>,
    /// The note as Markdown.
    content: String,
}

pub(crate) struct ProposeNote;

#[async_trait]
impl<I: From<NoteProposal> + Send + 'static> TypedTool<I> for ProposeNote {
    type Args = ProposeNoteArgs;

    fn name(&self) -> &'static str {
        "propose_note"
    }

    fn description(&self) -> &'static str {
        "Proposes a new Markdown note. The person reviews it before anything is saved; propose \
         only when they ask for a note."
    }

    async fn run_typed(&self, ctx: &ToolContext<'_>, args: ProposeNoteArgs) -> ToolOutcome<I> {
        const STEP: &str = "Proposed a note";
        if args.content.trim().is_empty() {
            return blank(STEP, "content");
        }
        let call = NoteToolCall::Propose {
            path: None,
            title: args.title,
            content: args.content,
        };
        notes(ctx, STEP, call).await
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ProposeNoteUpdateArgs {
    /// The note's path, as search_notes gave it.
    path: String,
    /// The complete new Markdown of the note.
    content: String,
}

pub(crate) struct ProposeNoteUpdate;

#[async_trait]
impl<I: From<NoteProposal> + Send + 'static> TypedTool<I> for ProposeNoteUpdate {
    type Args = ProposeNoteUpdateArgs;

    fn name(&self) -> &'static str {
        "propose_note_update"
    }

    fn description(&self) -> &'static str {
        "Proposes new text for an existing note. The person reviews it as a diff before anything \
         is saved; propose only when they ask or the note is clearly outdated."
    }

    async fn run_typed(
        &self,
        ctx: &ToolContext<'_>,
        args: ProposeNoteUpdateArgs,
    ) -> ToolOutcome<I> {
        const STEP: &str = "Proposed a note update";
        if args.path.trim().is_empty() {
            return blank(STEP, "path");
        }
        if args.content.trim().is_empty() {
            return blank(STEP, "content");
        }
        let call = NoteToolCall::Propose {
            path: Some(args.path),
            title: None,
            content: args.content,
        };
        notes(ctx, STEP, call).await
    }
}
