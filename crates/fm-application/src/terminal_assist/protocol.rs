//! The text protocol between the model and the terminal agent (task 0336). The model answers in
//! plain text; to use a tool it writes one line `TOOL {json}`. This works with every chat
//! endpoint, including local models without native tool calling.

use serde::Deserialize;

/// One tool the model asked for.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "tool", rename_all = "snake_case")]
pub(super) enum ToolCall {
    ListDir {
        path: String,
    },
    ReadFile {
        path: String,
    },
    Grep {
        pattern: String,
        #[serde(default)]
        path: Option<String>,
    },
    RunCommand {
        command: String,
    },
}

/// The tool call in a model reply, when the reply contains a `TOOL {json}` line.
pub(super) fn parse_tool_call(reply: &str) -> Option<ToolCall> {
    let json = reply
        .lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix("TOOL "))?;
    let call: ToolCall = serde_json::from_str(json.trim()).ok()?;
    match &call {
        // One approval must run exactly one line: a newline or control byte would run more.
        ToolCall::RunCommand { command }
            if command.trim().is_empty() || command.chars().any(char::is_control) =>
        {
            None
        }
        ToolCall::ListDir { path } | ToolCall::ReadFile { path } if path.trim().is_empty() => None,
        ToolCall::Grep { pattern, .. } if pattern.is_empty() => None,
        _ => Some(call),
    }
}

#[cfg(test)]
mod tests;
