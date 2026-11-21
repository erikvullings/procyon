//! Turning a terminal assistant session into a draft note (task 0337). Session text and command
//! output are redacted before the model sees them, and the generated note is redacted again,
//! because a model may repeat or rebuild a secret. The draft is only proposed: nothing is written.

use chrono::{DateTime, SecondsFormat, Utc};
use fm_transport_dto::{ChatFailureDto, NoteDraftDto, TerminalBlockDto};
use tokio_util::sync::CancellationToken;

use super::format::{NoteMeta, render_note, slugify, split_note};
use crate::chat::ChatGenerator;
use crate::file_assistant::truncate_to_bytes;
use crate::llm_profiles::{LlmChatGeneration, LlmProfileError, LlmRole};
use crate::terminal_assist::redact_secrets;

/// Most prompt bytes of conversation and commands together.
pub(crate) const MAX_PROMPT_BYTES: usize = 32 * 1024;
/// Most conversation bytes; the newest turns are kept.
const MAX_CONVERSATION_BYTES: usize = MAX_PROMPT_BYTES / 2;
/// Most bytes of one command's output: its start and, mostly, its end.
const MAX_OUTPUT_HEAD_BYTES: usize = 1024;
const MAX_OUTPUT_TAIL_BYTES: usize = 2048;
/// Upper bound on generated tokens for one note.
const MAX_NOTE_TOKENS: u32 = 2048;
/// Most tags kept.
const MAX_TAGS: usize = 8;
/// The title when nothing better is known.
const FALLBACK_TITLE: &str = "Terminal session";
/// What the redaction marks start with; each one is a value the person should check.
const REDACTION_MARK: &str = "[redacted";

pub(crate) const CAPTURE_PROMPT: &str = "You turn a terminal session into a short runbook the \
     person can follow next time they do this task, which they do rarely. Reply in Markdown in \
     exactly this shape:\n\
     Title: <a short title naming the task>\n\
     Tags: <two to five lowercase keywords, comma separated>\n\
     \n\
     ## Goal\n<one or two sentences>\n\
     ## Prerequisites\n<what must be in place; leave the section out when nothing is needed>\n\
     ## Steps\n<numbered steps; every command in a ```bash fence>\n\
     ## Verify\n<how to check it worked>\n\
     ## Pitfalls\n<failed attempts from the session (non-zero exit status) and how they were \
     fixed; leave the section out when there were none>\n\
     Keep only the commands that worked, in order. Replace values that are specific to this \
     machine with placeholders such as <domain> or <cert-path> when that is clearly right. Never \
     write passwords, keys or tokens: write <secret> instead. Use only what the session shows.";

/// What a draft is made from.
#[derive(Debug, Clone)]
pub(crate) struct CaptureInput {
    /// The assistant conversation, as the session renders it.
    pub(crate) conversation: String,
    /// Finished commands, oldest first.
    pub(crate) blocks: Vec<TerminalBlockDto>,
    /// Where the shell was working, as a location URI.
    pub(crate) working_directory: String,
    pub(crate) shell: Option<String>,
    pub(crate) host: Option<String>,
    pub(crate) title_hint: Option<String>,
    pub(crate) now: DateTime<Utc>,
}

/// Asks the chat role to write a runbook from the session and returns it for review.
pub(crate) async fn draft_note(
    generator: &impl ChatGenerator,
    input: CaptureInput,
) -> Result<NoteDraftDto, ChatFailureDto> {
    if input.conversation.trim().is_empty() && input.blocks.is_empty() {
        return Err(ChatFailureDto::InvalidRequest);
    }
    let raw = generator
        .generate_for_role(
            LlmRole::Chat,
            LlmChatGeneration {
                system_prompt: CAPTURE_PROMPT.to_owned(),
                user_prompt: capture_prompt(&input),
                images: Vec::new(),
                maximum_tokens: MAX_NOTE_TOKENS,
                context_window: None,
                temperature: None,
            },
            &CancellationToken::new(),
            &mut |_: &str| {},
        )
        .await
        .map_err(failure)?;
    if raw.trim().is_empty() {
        return Err(ChatFailureDto::Empty);
    }
    Ok(assemble(&input, &raw))
}

/// The session as the model sees it: environment, conversation and commands, all redacted.
fn capture_prompt(input: &CaptureInput) -> String {
    let mut prompt = String::new();
    if let Some(shell) = input.shell.as_deref().filter(|shell| !shell.is_empty()) {
        prompt.push_str(&format!("Shell: {shell}\n"));
    }
    if let Some(host) = input.host.as_deref().filter(|host| !host.is_empty()) {
        prompt.push_str(&format!("Machine: {host}\n"));
    }
    prompt.push_str(&format!(
        "Working directory: {}\n",
        shown_directory(&input.working_directory)
    ));
    let conversation = redact_secrets(newest_bytes(
        input.conversation.trim(),
        MAX_CONVERSATION_BYTES,
    ));
    if !conversation.trim().is_empty() {
        prompt.push_str("\nConversation with the assistant:\n");
        prompt.push_str(conversation.trim_end());
        prompt.push('\n');
    }
    let commands = commands_text(&input.blocks, MAX_PROMPT_BYTES - conversation.len());
    if !commands.is_empty() {
        prompt.push_str("\nCommands run, oldest first:\n");
        prompt.push_str(&commands);
    }
    prompt
}

/// The newest commands that fit in `budget` bytes, oldest first, each with bounded output.
fn commands_text(blocks: &[TerminalBlockDto], budget: usize) -> String {
    let mut kept: Vec<String> = Vec::new();
    let mut used = 0;
    for block in blocks.iter().rev() {
        let output = bounded_output(block.output.trim_end());
        let text = redact_secrets(&format!(
            "$ {}\n(exit status {})\n{}\n",
            block.command.trim(),
            block.exit_code,
            output
        ));
        if used + text.len() > budget {
            break;
        }
        used += text.len();
        kept.push(text);
    }
    kept.reverse();
    kept.concat()
}

/// The start and end of a long output, with a marker where the middle was left out.
fn bounded_output(output: &str) -> String {
    if output.len() <= MAX_OUTPUT_HEAD_BYTES + MAX_OUTPUT_TAIL_BYTES {
        return output.to_owned();
    }
    format!(
        "{}\n[...]\n{}",
        truncate_to_bytes(output, MAX_OUTPUT_HEAD_BYTES),
        newest_bytes(output, MAX_OUTPUT_TAIL_BYTES)
    )
}

/// The last `max_bytes` of `text`, starting on a character boundary.
fn newest_bytes(text: &str, max_bytes: usize) -> &str {
    if text.len() <= max_bytes {
        return text;
    }
    let mut start = text.len() - max_bytes;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    &text[start..]
}

/// A local working directory as a path; any other location as its URI.
fn shown_directory(uri: &str) -> String {
    match url::Url::parse(uri) {
        Ok(url) if url.scheme() == "file" => percent_encoding::percent_decode_str(url.path())
            .decode_utf8_lossy()
            .into_owned(),
        _ => uri.to_owned(),
    }
}

/// The model's reply as a complete note: front matter filled in, title heading first, redacted.
fn assemble(input: &CaptureInput, raw: &str) -> NoteDraftDto {
    let reply = unfenced(raw.trim());
    let (model_title, model_tags, body) = read_reply(reply);
    let heading = body.lines().find_map(|line| {
        line.strip_prefix("# ")
            .map(str::trim)
            .filter(|heading| !heading.is_empty())
    });
    let title = input
        .title_hint
        .as_deref()
        .map(str::trim)
        .filter(|hint| !hint.is_empty())
        .map(str::to_owned)
        .or(model_title)
        .or_else(|| heading.map(str::to_owned))
        .unwrap_or_else(|| FALLBACK_TITLE.to_owned());
    let body = with_title_heading(body.trim(), &title);
    let tags = clean_tags(model_tags);
    let stamp = input.now.to_rfc3339_opts(SecondsFormat::Secs, true);
    let meta = NoteMeta {
        title: Some(title.clone()),
        created: Some(stamp.clone()),
        updated: Some(stamp),
        tags: tags.clone(),
        hosts: input
            .host
            .iter()
            .filter(|host| !host.is_empty())
            .cloned()
            .collect(),
        shell: input.shell.clone().filter(|shell| !shell.is_empty()),
        source: Some("terminal-session".to_owned()),
    };
    let content = redact_secrets(&render_note(&meta, &body));
    let redactions = u32::try_from(content.matches(REDACTION_MARK).count()).unwrap_or(u32::MAX);
    NoteDraftDto {
        file_name: format!("{}.md", slugify(&title)),
        title,
        tags,
        content,
        redactions,
    }
}

/// The reply without a code fence wrapped around all of it.
fn unfenced(reply: &str) -> &str {
    let Some(rest) = reply.strip_prefix("```") else {
        return reply;
    };
    let Some(inner) = rest.strip_suffix("```") else {
        return reply;
    };
    // Drop the language tag on the opening line.
    inner
        .split_once('\n')
        .map_or(inner, |(_, body)| body)
        .trim()
}

/// Title, tags and body from either front matter or leading `Title:` / `Tags:` lines.
fn read_reply(reply: &str) -> (Option<String>, Vec<String>, String) {
    if let (Some(meta), body) = split_note(reply) {
        return (meta.title, meta.tags, body.to_owned());
    }
    let mut title = None;
    let mut tags = Vec::new();
    let mut body_start = 0;
    // Leading `Title:` / `Tags:` lines and the blank lines among them are the header.
    for line in reply.split_inclusive('\n') {
        let trimmed = line.trim();
        let lower = trimmed.to_ascii_lowercase();
        if lower.starts_with("title:") {
            title = Some(trimmed[6..].trim().trim_matches('"').to_owned())
                .filter(|title| !title.is_empty());
        } else if lower.starts_with("tags:") {
            tags = trimmed[5..]
                .trim()
                .trim_matches(['[', ']'])
                .split(',')
                .map(str::to_owned)
                .collect();
        } else if !trimmed.is_empty() {
            break;
        }
        body_start += line.len();
    }
    (title, tags, reply[body_start..].to_owned())
}

/// `body` starting with `# title`: an existing top heading is kept, otherwise one is added.
fn with_title_heading(body: &str, title: &str) -> String {
    let first = body.lines().find(|line| !line.trim().is_empty());
    if first.is_some_and(|line| line.starts_with("# ")) {
        format!("{body}\n")
    } else if body.is_empty() {
        format!("# {title}\n")
    } else {
        format!("# {title}\n\n{body}\n")
    }
}

/// Lowercase, `#`-free, unique, bounded tags.
fn clean_tags(tags: Vec<String>) -> Vec<String> {
    let mut clean: Vec<String> = Vec::new();
    for tag in tags {
        let tag = tag.trim().trim_start_matches('#').trim().to_lowercase();
        if !tag.is_empty() && !clean.contains(&tag) {
            clean.push(tag);
        }
        if clean.len() == MAX_TAGS {
            break;
        }
    }
    clean
}

fn failure(error: LlmProfileError) -> ChatFailureDto {
    match error {
        LlmProfileError::ConsentRequired(_) => ChatFailureDto::ConsentRequired,
        LlmProfileError::RoleNotAssigned => ChatFailureDto::RoleNotAssigned,
        LlmProfileError::Cancelled => ChatFailureDto::Cancelled,
        _ => ChatFailureDto::Unavailable,
    }
}

#[cfg(test)]
mod tests;
