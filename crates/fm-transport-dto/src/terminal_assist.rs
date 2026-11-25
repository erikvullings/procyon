//! Transport types for terminal assistance (task 0333, phase 1): one-shot questions about the
//! embedded terminal, answered through the chat role (0322). Nothing is saved, and the model
//! answers in text only; the person decides whether a suggested command is run.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::NoteProposalDto;

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

/// The person's answer to a request to create files (task 0338).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TerminalWriteDecisionDto {
    /// Allow files in the output folder for the rest of this question.
    Question,
    /// Allow files in the output folder for the rest of this terminal session.
    Session,
    /// Create nothing; the assistant stops.
    Decline,
}

/// A file the assistant wants to create, waiting for the person's approval (task 0338).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalWriteRequestDto {
    /// The output folder as shown to the person, for example `./summaries/`.
    pub folder: String,
    /// The first file's name.
    pub name: String,
    /// The first file's size in bytes.
    pub bytes: u32,
}

/// A change to an existing file, waiting for the person's approval (task 0352).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalEditRequestDto {
    /// The file, relative to the working folder.
    pub path: String,
    /// The change as a unified diff.
    pub diff: String,
    /// Lines added.
    pub added: u32,
    /// Lines removed.
    pub removed: u32,
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
    /// The session's trusted root (task 0343): the folder the session started in. When the
    /// working folder lies inside it, the tools, `@` mentions, skills and instructions use it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<String>,
    /// A skill the person invoked with `/name` (task 0343); its instructions go with the question.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill: Option<String>,
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
    /// The answer to a `writeRequest` of the previous reply (task 0338).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub write_decision: Option<TerminalWriteDecisionDto>,
    /// The answer to an `editRequest` of the previous reply (task 0352): `question` allows the
    /// later edits of this question too, `session` those of the whole session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edit_decision: Option<TerminalWriteDecisionDto>,
    /// Whether the assistant may offer to create files (the terminal setting, task 0338).
    #[serde(default)]
    pub allow_file_writes: bool,
    /// The session's auto-approve mode is on (task 0356): files are created and edits applied
    /// without asking. Commands are approved by the terminal, which still asks for dangerous ones.
    #[serde(default)]
    pub auto_approve: bool,
    /// The active workspace, whose semantic consent lets `search_notes` use the notes index
    /// (task 0337). Without it notes are searched by text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<Uuid>,
    /// Whether the assistant may read public web pages with `web_fetch` (task 0363): the setting,
    /// unless the session turned it off with `/web off`.
    #[serde(default)]
    pub allow_web: bool,
    /// The session's thinking effort (`/effort`, task 0361); absent uses the chat role's default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking: Option<crate::ThinkingEffortDto>,
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
    /// A note the assistant proposes to create or change (task 0337). Nothing is written until
    /// the person reviews and saves it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note_proposal: Option<NoteProposalDto>,
    /// Files the assistant wants to create (task 0338). Nothing is written until the person
    /// answers with a `writeDecision`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub write_request: Option<TerminalWriteRequestDto>,
    /// A change to an existing file (task 0352). Nothing is written until the person answers
    /// with an `editDecision`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edit_request: Option<TerminalEditRequestDto>,
    /// What `run_command` does, for the approval (task 0364).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_details: Option<TerminalCommandDetailsDto>,
}

/// What a command the assistant asks to run does (task 0364): the model's explanation and the
/// scripts it runs from the working folder, so the terminal can check them itself. A description
/// independent of the conversation is asked for separately.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalCommandDetailsDto {
    /// One or two sentences from the model: what the command does and why.
    pub explanation: String,
    /// Script files the command runs, read from the working folder.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scripts: Vec<TerminalScriptDto>,
}

/// A script file a command runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalScriptDto {
    /// The path as the command names it.
    pub path: String,
    /// Its text, as far as it was read.
    pub text: String,
    /// Whether the file goes on past `text`.
    pub truncated: bool,
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
    /// What a long tool call is doing now, for example `Summarising a.pdf: part 2 of 5`. Shown
    /// until the next event; not part of the answer.
    Progress {
        /// The one-line description.
        text: String,
    },
    /// A message the person sent while the assistant worked reached the model (task 0355).
    Steered {
        /// The message as sent.
        text: String,
    },
}

/// A saved assistant session, for `/resume` (task 0354).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalSavedSessionDto {
    /// The session to resume.
    pub id: String,
    /// Its first question, shortened.
    pub title: String,
    /// Its folder, as a location URI.
    pub folder: String,
    /// When it last changed, as an RFC 3339 timestamp.
    pub updated_at: String,
    /// How many questions it holds.
    pub questions: u32,
}

/// The files under a session's root, for `@` completion (task 0343).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalAssistFilesDto {
    /// Paths relative to the root with `/` separators; folders end with `/`.
    pub files: Vec<String>,
    /// Whether the listing stopped at its bound.
    pub truncated: bool,
}

/// A skill offered for `/` completion (task 0343).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalAssistSkillDto {
    /// The name typed after `/`.
    pub name: String,
    /// When to use it.
    pub description: String,
    /// `project`, `user` or `note`.
    pub source: String,
}
