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
}

/// The assistant's answer as plain text. For `command`, the text is exactly one command line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalAssistReplyDto {
    /// The answer.
    pub text: String,
}
