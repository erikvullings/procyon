//! What the terminal assistant adds to the shared agent tools (tasks 0336, 0338, 0339): file
//! access inside its working folder (local or over SSH, with documents read as text), and the
//! create-only `write_file` tool.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};

use super::documents::Documents;
use super::tools::{FileCall, ToolHost, ToolOutput};
use fm_agent::{
    ApprovalRequest, FileTools, ToolContext, ToolOutcome, ToolRegistry, Typed, TypedTool,
    command_registry,
};

/// The registry of the terminal assistant: the shared read-only tools, `run_command`, and
/// `write_file`. `write_file` is only described to the model when the person allows file
/// creation; a call to it is still understood and answered when it is not allowed.
pub(super) fn registry(describe_write_file: bool) -> ToolRegistry {
    let mut registry = command_registry();
    if describe_write_file {
        registry.register(Typed(WriteFile));
    }
    registry
}

/// The working folder as the file tools see it.
pub(super) struct TerminalFiles<'a> {
    pub(super) host: Option<&'a ToolHost>,
    pub(super) documents: &'a Documents,
}

impl TerminalFiles<'_> {
    async fn run(&self, call: FileCall) -> ToolOutcome {
        let output = match self.host {
            Some(host) => match self.documents.run(host, &call).await {
                Some(output) => output,
                None => host.run(call).await,
            },
            None => ToolOutput {
                step: "Looked up a file".to_owned(),
                text: "Error: file tools are not available for this location.".to_owned(),
            },
        };
        ToolOutcome::output(output.step, output.text)
    }
}

#[async_trait]
impl FileTools for TerminalFiles<'_> {
    async fn list_dir(&self, path: &str) -> ToolOutcome {
        self.run(FileCall::ListDir {
            path: path.to_owned(),
        })
        .await
    }

    async fn read_file(&self, path: &str, offset: usize) -> ToolOutcome {
        self.run(FileCall::ReadFile {
            path: path.to_owned(),
            offset: Some(offset),
        })
        .await
    }

    async fn grep(&self, pattern: &str, path: Option<&str>) -> ToolOutcome {
        self.run(FileCall::Grep {
            pattern: pattern.to_owned(),
            path: path.map(str::to_owned),
        })
        .await
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct WriteFileArgs {
    path: String,
    content: String,
}

/// Creates a new `.md` or `.txt` file in the output folder, after the person's approval. The
/// loop checks the name and the limits and asks.
struct WriteFile;

#[async_trait]
impl TypedTool for WriteFile {
    type Args = WriteFileArgs;

    fn name(&self) -> &'static str {
        "write_file"
    }

    fn description(&self) -> &'static str {
        // The folder name is `OUTPUT_FOLDER`; a test keeps the two in step.
        "creates a Markdown or text file in the folder summaries/ next to the working folder, \
         after the person approves. Use a short plain file name ending in .md or .txt, without \
         folders. It only creates new files and never changes an existing one: a taken name \
         gets a number, and the result tells you the final name. To summarise several \
         documents, read one and then write its file, one file per document, with a factual \
         summary. Use it only when the person asks for files."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {"path": {"type": "string"}, "content": {"type": "string"}},
            "required": ["path", "content"],
        })
    }

    async fn run_typed(&self, _ctx: &ToolContext<'_>, args: WriteFileArgs) -> ToolOutcome {
        if args.path.trim().is_empty() {
            return ToolOutcome::output("Create file", "Error: path must not be empty.");
        }
        ToolOutcome::NeedsApproval(ApprovalRequest::WriteFile {
            name: args.path.trim().to_owned(),
            content: args.content,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::write::OUTPUT_FOLDER;
    use super::*;

    #[test]
    fn the_description_names_the_output_folder() {
        assert!(
            WriteFile
                .description()
                .contains(&format!("{OUTPUT_FOLDER}/"))
        );
    }
}
