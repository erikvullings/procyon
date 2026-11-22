//! The tools shared by the agents (task 0339): looking at files, the notes, and the person's
//! terminal. Each is one definition: its name, what it does for the prompt, its arguments, and
//! its run. File and notes access come through the [`ToolContext`], so each host decides what a
//! path means and what is allowed.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{ApprovalRequest, ToolContext, ToolOutcome, TypedTool};
use crate::terminal_assist::{NoteToolCall, NoteToolOutcome};

fn unavailable(step: &str, what: &str) -> ToolOutcome {
    ToolOutcome::output(step, format!("Error: {what} are not available here."))
}

fn blank(step: &str, what: &str) -> ToolOutcome {
    ToolOutcome::output(step, format!("Error: {what} must not be empty."))
}

#[derive(Debug, Deserialize)]
pub(crate) struct PathArgs {
    path: String,
}

pub(crate) struct ListDir;

#[async_trait]
impl TypedTool for ListDir {
    type Args = PathArgs;

    fn name(&self) -> &'static str {
        "list_dir"
    }

    fn description(&self) -> &'static str {
        "lists a folder. Paths are relative to the working folder and cannot leave it."
    }

    fn input_schema(&self) -> Value {
        json!({"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]})
    }

    async fn run_typed(&self, ctx: &ToolContext<'_>, args: PathArgs) -> ToolOutcome {
        if args.path.trim().is_empty() {
            return blank("Listed a folder", "path");
        }
        match ctx.files {
            Some(files) => files.list_dir(&args.path).await,
            None => unavailable("Looked up a file", "file tools"),
        }
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct ReadFileArgs {
    path: String,
    /// Byte offset to continue a file that an earlier read cut off.
    offset: Option<usize>,
}

pub(crate) struct ReadFile;

#[async_trait]
impl TypedTool for ReadFile {
    type Args = ReadFileArgs;

    fn name(&self) -> &'static str {
        "read_file"
    }

    fn description(&self) -> &'static str {
        "reads a text file, or the text of an EPUB, Word, PDF, PowerPoint or Excel document. A \
         long file is cut off with a note: continue with the offset it names."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {"path": {"type": "string"}, "offset": {"type": "integer"}},
            "required": ["path"],
        })
    }

    async fn run_typed(&self, ctx: &ToolContext<'_>, args: ReadFileArgs) -> ToolOutcome {
        if args.path.trim().is_empty() {
            return blank("Read a file", "path");
        }
        match ctx.files {
            Some(files) => files.read_file(&args.path, args.offset.unwrap_or(0)).await,
            None => unavailable("Looked up a file", "file tools"),
        }
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct GrepArgs {
    pattern: String,
    path: Option<String>,
}

pub(crate) struct Grep;

#[async_trait]
impl TypedTool for Grep {
    type Args = GrepArgs;

    fn name(&self) -> &'static str {
        "grep"
    }

    fn description(&self) -> &'static str {
        "finds lines containing text, in a folder or file (path defaults to the working folder)."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {"pattern": {"type": "string"}, "path": {"type": "string"}},
            "required": ["pattern"],
        })
    }

    async fn run_typed(&self, ctx: &ToolContext<'_>, args: GrepArgs) -> ToolOutcome {
        if args.pattern.is_empty() {
            return blank("Searched files", "pattern");
        }
        match ctx.files {
            Some(files) => files.grep(&args.pattern, args.path.as_deref()).await,
            None => unavailable("Looked up a file", "file tools"),
        }
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct RunCommandArgs {
    command: String,
}

/// Asks the person to run one command; the loop does the asking.
pub(crate) struct RunCommand;

#[async_trait]
impl TypedTool for RunCommand {
    type Args = RunCommandArgs;

    fn name(&self) -> &'static str {
        "run_command"
    }

    fn description(&self) -> &'static str {
        "asks the person to run a command in their terminal. They may decline, and the result \
         returns with its exit status and output."
    }

    fn input_schema(&self) -> Value {
        json!({"type": "object", "properties": {"command": {"type": "string"}}, "required": ["command"]})
    }

    async fn run_typed(&self, _ctx: &ToolContext<'_>, args: RunCommandArgs) -> ToolOutcome {
        // One approval must run exactly one line: a newline or control byte would run more.
        if args.command.trim().is_empty() || args.command.chars().any(char::is_control) {
            return ToolOutcome::output(
                "Run command",
                "Error: the command must be one non-empty line without control characters.",
            );
        }
        ToolOutcome::NeedsApproval(ApprovalRequest::RunCommand {
            command: args.command.trim().to_owned(),
        })
    }
}

/// Runs a notes call through the notes capability, or explains that there is none.
async fn notes(ctx: &ToolContext<'_>, step: &str, call: NoteToolCall) -> ToolOutcome {
    let Some(notes) = ctx.notes else {
        return ToolOutcome::output(step, "Error: notes are not available here.");
    };
    match notes.run(call).await {
        NoteToolOutcome::Output { step, text } => ToolOutcome::Output { step, text },
        NoteToolOutcome::Proposal(proposal) => ToolOutcome::Proposal(proposal),
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct SearchNotesArgs {
    query: String,
    limit: Option<u32>,
}

pub(crate) struct SearchNotes;

#[async_trait]
impl TypedTool for SearchNotes {
    type Args = SearchNotesArgs;

    fn name(&self) -> &'static str {
        "search_notes"
    }

    fn description(&self) -> &'static str {
        "searches the person's notes (runbooks for tasks they do rarely). Search them before \
         answering a how-to question."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {"query": {"type": "string"}, "limit": {"type": "integer"}},
            "required": ["query"],
        })
    }

    async fn run_typed(&self, ctx: &ToolContext<'_>, args: SearchNotesArgs) -> ToolOutcome {
        if args.query.trim().is_empty() {
            return blank("Looked in the notes", "query");
        }
        let call = NoteToolCall::Search {
            query: args.query,
            limit: args.limit,
        };
        notes(ctx, "Looked in the notes", call).await
    }
}

pub(crate) struct ReadNote;

#[async_trait]
impl TypedTool for ReadNote {
    type Args = PathArgs;

    fn name(&self) -> &'static str {
        "read_note"
    }

    fn description(&self) -> &'static str {
        "reads one note by the path search_notes gave."
    }

    fn input_schema(&self) -> Value {
        json!({"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]})
    }

    async fn run_typed(&self, ctx: &ToolContext<'_>, args: PathArgs) -> ToolOutcome {
        if args.path.trim().is_empty() {
            return blank("Read a note", "path");
        }
        notes(ctx, "Read a note", NoteToolCall::Read { path: args.path }).await
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct ProposeNoteArgs {
    title: Option<String>,
    content: String,
}

pub(crate) struct ProposeNote;

#[async_trait]
impl TypedTool for ProposeNote {
    type Args = ProposeNoteArgs;

    fn name(&self) -> &'static str {
        "propose_note"
    }

    fn description(&self) -> &'static str {
        "proposes a new Markdown note. The person reviews it before anything is saved; propose \
         only when they ask for a note."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {"title": {"type": "string"}, "content": {"type": "string"}},
            "required": ["content"],
        })
    }

    async fn run_typed(&self, ctx: &ToolContext<'_>, args: ProposeNoteArgs) -> ToolOutcome {
        if args.content.trim().is_empty() {
            return blank("Proposed a note", "content");
        }
        let call = NoteToolCall::Propose {
            path: None,
            title: args.title,
            content: args.content,
        };
        notes(ctx, "Proposed a note", call).await
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct ProposeNoteUpdateArgs {
    path: String,
    content: String,
}

pub(crate) struct ProposeNoteUpdate;

#[async_trait]
impl TypedTool for ProposeNoteUpdate {
    type Args = ProposeNoteUpdateArgs;

    fn name(&self) -> &'static str {
        "propose_note_update"
    }

    fn description(&self) -> &'static str {
        "proposes new text for an existing note. The person reviews it as a diff before anything \
         is saved; propose only when they ask or the note is clearly outdated."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {"path": {"type": "string"}, "content": {"type": "string"}},
            "required": ["path", "content"],
        })
    }

    async fn run_typed(&self, ctx: &ToolContext<'_>, args: ProposeNoteUpdateArgs) -> ToolOutcome {
        if args.path.trim().is_empty() {
            return blank("Proposed a note update", "path");
        }
        if args.content.trim().is_empty() {
            return blank("Proposed a note update", "content");
        }
        let call = NoteToolCall::Propose {
            path: Some(args.path),
            title: None,
            content: args.content,
        };
        notes(ctx, "Proposed a note update", call).await
    }
}
