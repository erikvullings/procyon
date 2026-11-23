//! The conversation of an agent as messages an endpoint can take natively: the person's messages,
//! the model's replies with the calls they made, one result per call, and notes from the host.
//!
//! An endpoint that calls tools natively gets these as structured messages (an assistant message
//! with its calls, then a tool message per call id). For the text protocol a host renders them
//! with [`render_text`]. [`fit_messages`] bounds a conversation without breaking a reply apart
//! from its results, which a native endpoint would refuse.

use std::collections::HashSet;

use serde_json::Value;

/// One call the model made, as the conversation records it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCallRecord {
    /// The endpoint's id for the call, or one the loop made up; results refer to it.
    pub id: String,
    /// The tool's name.
    pub name: String,
    /// The arguments as the model wrote them (a JSON object).
    pub arguments: Value,
}

/// One message of an agent conversation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    /// What the person said (with any context the host attached), as the host words it.
    User(String),
    /// A reply of the model: its text as written, and the calls the loop runs from it.
    Assistant {
        /// The reply as the model wrote it, call lines included.
        text: String,
        /// The calls taken from the reply, in order.
        calls: Vec<ToolCallRecord>,
    },
    /// The result of one call.
    ToolResult {
        /// The [`ToolCallRecord::id`] this answers.
        call_id: String,
        /// The tool's name (Ollama wants it on the result).
        name: String,
        /// What the model sees.
        text: String,
    },
    /// A message from the host or the loop to the model (a nudge, a note that results were left
    /// out); a user message for a native endpoint.
    Note(String),
}

/// Put in place of messages that no longer fit.
pub const OMITTED_NOTE: &str = "[Earlier tool results were left out to save space.]";
/// The result of a call that never ran (the person moved on, or its result was lost).
pub const NOT_RUN_RESULT: &str = "This call was not run.";

impl Message {
    /// The result of `call`.
    pub fn result(call: &ToolCallRecord, text: impl Into<String>) -> Self {
        Self::ToolResult {
            call_id: call.id.clone(),
            name: call.name.clone(),
            text: text.into(),
        }
    }

    /// About how many bytes the message takes in a prompt.
    pub fn size(&self) -> usize {
        match self {
            Self::User(text) | Self::Note(text) => text.len(),
            Self::Assistant { text, calls } => {
                text.len()
                    + calls
                        .iter()
                        .map(|call| call.name.len() + call.arguments.to_string().len() + 32)
                        .sum::<usize>()
            }
            Self::ToolResult { text, .. } => text.len() + 16,
        }
    }

    /// The reply's own words, without its `TOOL` lines: the content of a native assistant message,
    /// whose calls travel separately.
    pub fn prose(text: &str) -> String {
        let kept: Vec<&str> = text
            .lines()
            .filter(|line| {
                line.trim_start()
                    .strip_prefix("TOOL")
                    .is_none_or(|rest| !rest.starts_with([' ', '\t', '{']))
            })
            .collect();
        kept.join("\n").trim().to_owned()
    }
}

/// The conversation as one prompt body for the text protocol: `Assistant:` before replies,
/// `Tool result:` before results, user messages and notes as they are. A reply keeps its call
/// lines (native calls are written back into the reply as `TOOL` lines by the host).
pub fn render_text(messages: &[Message]) -> String {
    let mut text = String::with_capacity(messages.iter().map(Message::size).sum::<usize>() + 64);
    for message in messages {
        match message {
            Message::User(said) | Message::Note(said) => text.push_str(said.trim_end()),
            Message::Assistant { text: reply, .. } => {
                text.push_str("Assistant: ");
                text.push_str(reply.trim_end());
            }
            Message::ToolResult { text: result, .. } => {
                text.push_str("Tool result:\n");
                text.push_str(result.trim_end());
            }
        }
        text.push_str("\n\n");
    }
    text
}

/// Bounds a conversation to about `limit` bytes. The last user message (the current task) always
/// stays, and so does the newest group after it (the result the model waits for). Then the newest
/// groups after the task that fit, then the groups before it that fit; groups left out between the
/// task and the kept tail become one [`OMITTED_NOTE`]. A group is a reply with all its results:
/// it is kept or left out whole, a call without a result gets [`NOT_RUN_RESULT`], and a result
/// without its reply becomes a note.
pub fn fit_messages(messages: &[Message], limit: usize) -> Vec<Message> {
    let groups = group(messages);
    if groups.is_empty() {
        return Vec::new();
    }
    let size = |group: &Vec<Message>| group.iter().map(Message::size).sum::<usize>();
    let pin = groups
        .iter()
        .rposition(|group| matches!(group.first(), Some(Message::User(_))))
        .unwrap_or(0);
    let mut budget = limit.saturating_sub(size(&groups[pin]));
    let mut tail_start = groups.len();
    while tail_start > pin + 1 {
        let wanted = size(&groups[tail_start - 1]);
        // The newest group stays even when it alone is over the bound.
        if wanted > budget && tail_start < groups.len() {
            break;
        }
        budget = budget.saturating_sub(wanted);
        tail_start -= 1;
    }
    let mut head_start = pin;
    while head_start > 0 && size(&groups[head_start - 1]) <= budget {
        head_start -= 1;
        budget -= size(&groups[head_start]);
    }
    let mut kept: Vec<Message> = groups[head_start..=pin].concat();
    if tail_start > pin + 1 {
        kept.push(Message::Note(OMITTED_NOTE.to_owned()));
    }
    kept.extend(groups[tail_start..].concat());
    kept
}

/// Splits messages into groups: a reply with the results of its calls (in the reply's call order,
/// missing ones filled in), or a single other message.
fn group(messages: &[Message]) -> Vec<Vec<Message>> {
    let mut groups: Vec<Vec<Message>> = Vec::new();
    let mut index = 0;
    while index < messages.len() {
        match &messages[index] {
            Message::Assistant { calls, .. } if !calls.is_empty() => {
                let ids: HashSet<&str> = calls.iter().map(|call| call.id.as_str()).collect();
                let mut results = Vec::new();
                let mut next = index + 1;
                // The results may come later than right after the reply (a command the person ran,
                // with a note in between); only messages before the next reply or user message
                // belong to it.
                let mut between = Vec::new();
                while let Some(message) = messages.get(next) {
                    match message {
                        Message::ToolResult { call_id, .. } if ids.contains(call_id.as_str()) => {
                            results.push(message.clone());
                        }
                        Message::Note(_) => between.push(message.clone()),
                        _ => break,
                    }
                    next += 1;
                }
                let mut group = vec![messages[index].clone()];
                for call in calls {
                    let result = results
                        .iter()
                        .find(|result| {
                            matches!(result, Message::ToolResult { call_id, .. } if *call_id == call.id)
                        })
                        .cloned()
                        .unwrap_or_else(|| Message::result(call, NOT_RUN_RESULT));
                    group.push(result);
                }
                groups.push(group);
                groups.extend(between.into_iter().map(|note| vec![note]));
                index = next;
            }
            Message::ToolResult { text, .. } => {
                groups.push(vec![Message::Note(format!("Tool result:\n{text}"))]);
                index += 1;
            }
            message => {
                groups.push(vec![message.clone()]);
                index += 1;
            }
        }
    }
    groups
}

#[cfg(test)]
mod tests;
