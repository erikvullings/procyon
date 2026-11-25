//! What the terminal assistant adds to the shared agent tools (tasks 0336, 0338, 0339): the
//! create-only `write_file` tool, and the interrupts that stop a turn for the person.

use async_trait::async_trait;
use fm_agent::{
    CommandRequest, EditFile, EditRequest, NoteProposal, ToolContext, ToolOutcome, ToolRegistry,
    Typed, TypedTool, command_registry,
};
use schemars::JsonSchema;
use serde::Deserialize;

use super::session::PendingWrite;
use crate::agent_host::READ_FILE_DESCRIPTION;

/// Why an `ask` turn stopped for the person.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum TerminalInterrupt {
    /// A note to review.
    Note(NoteProposal),
    /// A command to approve and run in the terminal.
    Command(CommandRequest),
    /// A file to create, after the checks and the person's approval.
    WriteFile(PendingWrite),
    /// A change to an existing file, after the person's approval (task 0352).
    Edit(EditRequest),
}

impl From<EditRequest> for TerminalInterrupt {
    fn from(request: EditRequest) -> Self {
        Self::Edit(request)
    }
}

impl From<NoteProposal> for TerminalInterrupt {
    fn from(proposal: NoteProposal) -> Self {
        Self::Note(proposal)
    }
}

impl From<CommandRequest> for TerminalInterrupt {
    fn from(request: CommandRequest) -> Self {
        Self::Command(request)
    }
}

/// The registry of the terminal assistant: the shared read-only tools, `run_command`,
/// `write_file` and `edit_file`. The two writing tools are only described to the model when the
/// person allows file changes; a call to either is still understood and answered when not.
pub(super) fn registry(describe_write_file: bool) -> ToolRegistry<TerminalInterrupt> {
    let mut registry = command_registry();
    registry.describe("read_file", READ_FILE_DESCRIPTION);
    if describe_write_file {
        registry
            .register(Typed(WriteFile))
            .register(Typed(EditFile));
    }
    registry
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct WriteFileArgs {
    /// A short plain file name ending in .md or .txt, without folders.
    path: String,
    /// The file's text.
    content: String,
}

/// Creates a new `.md` or `.txt` file in the output folder, after the person's approval. The
/// caller checks the name and the limits and asks.
struct WriteFile;

#[async_trait]
impl TypedTool<TerminalInterrupt> for WriteFile {
    type Args = WriteFileArgs;

    fn argument_aliases(&self) -> &'static [(&'static str, &'static [&'static str])] {
        &[("path", &["file_path", "filename"])]
    }

    fn name(&self) -> &'static str {
        "write_file"
    }

    fn description(&self) -> &'static str {
        // The folder name is `OUTPUT_FOLDER`; a test keeps the two in step.
        "Creates a Markdown or text file in the folder summaries/ next to the working folder, \
         after the person approves. Use a short plain file name ending in .md or .txt, without \
         folders. It only creates new files and never changes an existing one: a taken name \
         gets a number, and the result tells you the final name. To summarise documents, use \
         summarize_file for each, then write the summaries; it keeps the documents out of the \
         conversation. Use it only when the person asks for files."
    }

    async fn run_typed(
        &self,
        _ctx: &ToolContext<'_>,
        args: WriteFileArgs,
    ) -> ToolOutcome<TerminalInterrupt> {
        if args.path.trim().is_empty() {
            return ToolOutcome::error("Create file", "path must not be empty.");
        }
        ToolOutcome::Interrupt(TerminalInterrupt::WriteFile(PendingWrite {
            name: args.path.trim().to_owned(),
            content: args.content,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::super::write::OUTPUT_FOLDER;
    use super::*;

    #[test]
    fn the_description_names_the_output_folder() {
        let description = TypedTool::<TerminalInterrupt>::description(&WriteFile);
        assert!(description.contains(&format!("{OUTPUT_FOLDER}/")));
    }

    #[test]
    fn write_file_is_described_only_when_allowed() {
        assert!(registry(true).contains("write_file"));
        assert!(!registry(false).contains("write_file"));
        assert!(registry(false).prompt_section().contains("PowerPoint"));
    }
}
