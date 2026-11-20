//! Transport types for terminal assistance (task 0333, phase 1): one-shot questions about the
//! embedded terminal, answered through the chat role (0322). Nothing is saved, and the model
//! answers in text only; the person decides whether a suggested command is run.

use serde::{Deserialize, Serialize};

/// What the person asked the assistant to do with the terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TerminalAssistKindDto {
    /// Turns a natural-language request into one shell command.
    Command,
    /// Explains the recent terminal output.
    Explain,
    /// Explains a failure in the recent output and proposes a corrected command.
    Fix,
    /// A free-form question in a session (task 0336). The model decides whether to answer, propose
    /// a command, look at files with read-only tools, or ask to run a command.
    Ask,
}

/// One finished command block from the terminal, as the shell integration marked it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalBlockDto {
    /// The command line that was run.
    pub command: String,
    /// Its exit status.
    pub exit_code: i32,
    /// Its output as shown on screen.
    pub output: String,
}

/// What the person lets the assistant see besides the question. Every field is optional: the
/// terminal shows each part as a chip, and removing a chip leaves that part out of the request.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalContextDto {
    /// Leaves the operating system and architecture out.
    #[serde(default)]
    pub omit_environment: bool,
    /// Names in the working folder, as the active pane lists them.
    #[serde(default)]
    pub listing: Vec<String>,
    /// Names selected in the active pane.
    #[serde(default)]
    pub selected: Vec<String>,
    /// The other pane's folder, as a location URI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub other_folder: Option<String>,
    /// Recent finished commands with their output, oldest first.
    #[serde(default)]
    pub blocks: Vec<TerminalBlockDto>,
    /// Files named with `@path` in the question; read by the host inside the working folder.
    #[serde(default)]
    pub files: Vec<String>,
}

/// The outcome of a command the assistant asked to run and the person approved or declined.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalToolResultDto {
    /// The command that was proposed.
    pub command: String,
    /// The person declined to run it. The assistant stops.
    #[serde(default)]
    pub declined: bool,
    /// Its exit status when it ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    /// Its output when it ran.
    #[serde(default)]
    pub output: String,
}

/// One terminal assistance request. The transcript is the recent output as shown on screen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalAssistRequestDto {
    /// What to do.
    pub kind: TerminalAssistKindDto,
    /// Recent terminal output, most recent last. Required for `explain` and `fix`.
    #[serde(default)]
    pub transcript: String,
    /// The person's description of the wanted command. Required for `command`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request: Option<String>,
    /// Where the terminal's shell is working, as the location URI.
    pub working_directory: String,
    /// The shell's name when known, for example `zsh`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shell: Option<String>,
    /// Identifies the terminal tab's session for `ask`; the host keeps its earlier turns until
    /// the session is closed. Required for `ask`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// What the person lets the assistant see.
    #[serde(default)]
    pub context: TerminalContextDto,
    /// The result of the command the previous reply asked to run. Only for `ask`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_result: Option<TerminalToolResultDto>,
}

/// The assistant's answer as plain text. For `command`, the text is exactly one command line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalAssistReplyDto {
    /// The answer. Empty when the reply is only a command waiting for approval or a stop.
    pub text: String,
    /// What the assistant looked at on the way, one short line each, for example `Read Cargo.toml`.
    #[serde(default)]
    pub steps: Vec<String>,
    /// A command the assistant asks to run. Nothing runs until the person approves; the result
    /// comes back as `toolResult` on the next request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_command: Option<String>,
}

/// Something shown while an `ask` is answered, so the terminal can write it as it happens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TerminalAssistEventDto {
    /// One finished line of the answer, without its line break.
    Line {
        /// The line's text, Markdown as the model wrote it.
        text: String,
    },
    /// Something the assistant looked at, for example `Read Cargo.toml`.
    Step {
        /// The one-line description.
        text: String,
    },
}
