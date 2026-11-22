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
        /// Byte offset to continue a file that an earlier read cut off.
        #[serde(default)]
        offset: Option<usize>,
    },
    Grep {
        pattern: String,
        #[serde(default)]
        path: Option<String>,
    },
    RunCommand {
        command: String,
    },
    /// Searches the person's notes (task 0337).
    SearchNotes {
        query: String,
        #[serde(default)]
        limit: Option<u32>,
    },
    /// Reads one note inside the notes folder.
    ReadNote {
        path: String,
    },
    /// Creates a new `.md` or `.txt` file in the output folder, after the person's approval
    /// (task 0338). `path` is a plain file name.
    WriteFile {
        path: String,
        content: String,
    },
    /// Proposes a new note for the person to review.
    ProposeNote {
        #[serde(default)]
        title: Option<String>,
        content: String,
    },
    /// Proposes new text for an existing note for the person to review.
    ProposeNoteUpdate {
        path: String,
        content: String,
    },
}

/// The tool call in a model reply. The documented form is a `TOOL {json}` line, but models also
/// wrap the object in `<tool_call>` tags or a code fence, or use `{"name":..,"arguments":{..}}`,
/// so the first JSON object anywhere in the reply that names a known tool is accepted.
pub(super) fn parse_tool_call(reply: &str) -> Option<ToolCall> {
    reply
        .match_indices('{')
        .find_map(|(start, _)| tool_call_at(&reply[start..]))
}

/// The tool call in the JSON object that starts `text`, when it is one.
fn tool_call_at(text: &str) -> Option<ToolCall> {
    let object = balanced_object(text)?;
    let mut value: serde_json::Value = serde_json::from_str(object).ok()?;
    if value.get("tool").is_none() {
        // `{"name": "read_file", "arguments": {"path": ".."}}`
        let name = value.get("name")?.as_str()?.to_owned();
        let mut flat = value.get("arguments")?.as_object()?.clone();
        flat.insert("tool".to_owned(), serde_json::Value::String(name));
        value = serde_json::Value::Object(flat);
    }
    let call: ToolCall = serde_json::from_value(value).ok()?;
    match &call {
        // One approval must run exactly one line: a newline or control byte would run more.
        ToolCall::RunCommand { command }
            if command.trim().is_empty() || command.chars().any(char::is_control) =>
        {
            None
        }
        ToolCall::ListDir { path }
        | ToolCall::ReadFile { path, .. }
        | ToolCall::ReadNote { path }
        | ToolCall::WriteFile { path, .. }
            if path.trim().is_empty() =>
        {
            None
        }
        ToolCall::SearchNotes { query, .. } if query.trim().is_empty() => None,
        ToolCall::ProposeNote { content, .. } | ToolCall::ProposeNoteUpdate { content, .. }
            if content.trim().is_empty() =>
        {
            None
        }
        ToolCall::ProposeNoteUpdate { path, .. } if path.trim().is_empty() => None,
        ToolCall::Grep { pattern, .. } if pattern.is_empty() => None,
        _ => Some(call),
    }
}

/// The text from the opening brace to its matching closing brace, braces in strings ignored.
fn balanced_object(text: &str) -> Option<&str> {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (index, character) in text.char_indices() {
        if in_string {
            match character {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match character {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(&text[..=index]);
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests;
