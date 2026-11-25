//! What the terminal's `@` and `/` completion and its session commands need (task 0343): the
//! files under a session's root, the skills that can be invoked, the instructions of an invoked
//! skill, and `/compact`.

use std::collections::VecDeque;

use fm_agent::{NoteToolCall, SkillLocation, ToolOutcome, redact_secrets};
use fm_domain::{EntryKind, Location};
use fm_transport_dto::{ChatFailureDto, TerminalAssistFilesDto, TerminalAssistSkillDto};
use fm_vfs::{ListOptions, ProviderRegistry};
use tokio_util::sync::CancellationToken;

use super::{Sessions, TerminalHost};
use crate::agent_host::{SkillScope, chat_failure, project_context};
use crate::chat::ChatGenerator;
use crate::file_assistant::truncate_to_bytes;
use crate::llm_profiles::{LlmChatGeneration, LlmRole, ReplyFormat};

/// Most paths listed under a root.
const MAX_FILES: usize = 5000;
/// Deepest folder level listed under a root.
const MAX_DEPTH: usize = 8;
/// Entries read from one folder.
const PAGE: usize = 1000;
/// Folders never listed: build output, dependencies and caches.
const SKIPPED_FOLDERS: [&str; 9] = [
    "node_modules",
    "target",
    "dist",
    "build",
    "out",
    "__pycache__",
    "venv",
    "vendor",
    "coverage",
];
/// Most notes offered for `/` completion.
const MAX_NOTE_SKILLS: usize = 200;
/// Longest skill text sent with an invoked skill, in bytes.
const MAX_SKILL_TEXT: usize = 16 * 1024;
/// Most bytes of conversation summarised by `/compact`.
const MAX_COMPACTED_BYTES: usize = 200 * 1024;

const COMPACT_PROMPT: &str = "You summarise a terminal assistant conversation so it can continue \
     from the summary alone. Keep the person's goals, the decisions made, the facts learned \
     (paths, commands, errors and their causes), files created or changed, and what is still to \
     do. Leave out greetings and anything superseded. Write plain text under 400 words.";

/// The paths under `root` (relative, `/`-separated, folders ending with `/`), breadth first and
/// bounded. Hidden entries, links and the usual build and dependency folders are left out.
pub(crate) async fn list_files(providers: &ProviderRegistry, root: &str) -> TerminalAssistFilesDto {
    let Ok(location) = providers.parse(root) else {
        return TerminalAssistFilesDto::default();
    };
    let Ok(provider) = providers.resolve(&location) else {
        return TerminalAssistFilesDto::default();
    };
    let mut files = Vec::new();
    let mut truncated = false;
    let mut pending: VecDeque<(Location, String, usize)> =
        VecDeque::from([(location, String::new(), 0)]);
    'walk: while let Some((folder, prefix, depth)) = pending.pop_front() {
        let Ok(page) = provider
            .list(
                &folder,
                ListOptions {
                    page_size: PAGE,
                    continuation_token: None,
                },
                CancellationToken::new(),
            )
            .await
        else {
            continue;
        };
        truncated |= page.has_more;
        let mut entries = page.entries;
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        for entry in entries {
            if entry.name.starts_with('.') || entry.hidden {
                continue;
            }
            if files.len() >= MAX_FILES {
                truncated = true;
                break 'walk;
            }
            let path = format!("{prefix}{}", entry.name);
            match entry.kind {
                EntryKind::Directory => {
                    if SKIPPED_FOLDERS.contains(&entry.name.as_str()) {
                        continue;
                    }
                    files.push(format!("{path}/"));
                    if depth < MAX_DEPTH {
                        pending.push_back((entry.location, format!("{path}/"), depth + 1));
                    } else {
                        truncated = true;
                    }
                }
                EntryKind::File => files.push(path),
                _ => {}
            }
        }
    }
    TerminalAssistFilesDto { files, truncated }
}

/// The skills `/` can invoke in `working_directory`: folder skills, then notes.
pub(crate) async fn list_skills(
    terminal: &TerminalHost<'_>,
    working_directory: &str,
) -> Vec<TerminalAssistSkillDto> {
    let project = project_context(
        terminal.skills,
        terminal.providers,
        None,
        working_directory,
        "",
        SkillScope::HostWide,
    )
    .await;
    let mut catalogue = project.skills;
    if let Some(notes) = terminal.notes {
        catalogue.add_notes(notes.list_skills(MAX_NOTE_SKILLS).await);
    }
    catalogue
        .skills()
        .iter()
        .map(|skill| TerminalAssistSkillDto {
            name: skill.name.clone(),
            description: skill.description.clone(),
            source: skill.source.label().to_owned(),
        })
        .collect()
}

/// The instructions of the skill invoked with `/name`, worded for the person's context; `None`
/// when there is no such skill.
pub(crate) async fn invoked_skill(
    terminal: &TerminalHost<'_>,
    working_directory: &str,
    name: &str,
) -> Option<String> {
    let name = name.trim().trim_start_matches('/');
    let project = project_context(
        terminal.skills,
        terminal.providers,
        terminal.notes,
        working_directory,
        name,
        SkillScope::HostWide,
    )
    .await;
    let mut catalogue = project.skills;
    if catalogue.find(name).is_none()
        && let Some(notes) = terminal.notes
    {
        catalogue.add_notes(notes.list_skills(MAX_NOTE_SKILLS).await);
    }
    let skill = catalogue.find(name)?;
    let text = match &skill.location {
        SkillLocation::Folder { .. } => skill.read_body().await?.ok()?,
        SkillLocation::Note(path) => {
            let notes = terminal.notes?;
            match notes.run(NoteToolCall::Read { path: path.clone() }).await {
                ToolOutcome::Output(output) => output.text,
                ToolOutcome::Interrupt(_) => return None,
            }
        }
    };
    Some(format!(
        "The person invoked the skill `{}` for this question; follow its instructions:\n{}",
        skill.name,
        redact_secrets(truncate_to_bytes(&text, MAX_SKILL_TEXT))
    ))
}

/// Replaces a session's conversation with a summary the model writes (`/compact`).
pub(crate) async fn compact(
    generator: &impl ChatGenerator,
    sessions: &Sessions,
    id: &str,
) -> Result<(), ChatFailureDto> {
    let conversation = sessions.render_within(id, MAX_COMPACTED_BYTES);
    if conversation.trim().is_empty() {
        return Ok(());
    }
    let summary = generator
        .generate_for_role(
            LlmRole::Chat,
            LlmChatGeneration {
                followups: Vec::new(),
                tools: Vec::new(),
                system_prompt: COMPACT_PROMPT.to_owned(),
                user_prompt: conversation,
                images: Vec::new(),
                maximum_tokens: None,
                context_window: None,
                temperature: None,
                reply: ReplyFormat::Text,
                thinking: crate::llm_profiles::ThinkingRequest::Background,
            },
            &CancellationToken::new(),
            &mut |_| {},
        )
        .await
        .map_err(chat_failure)?;
    let summary = summary.trim();
    if summary.is_empty() {
        return Err(ChatFailureDto::Empty);
    }
    sessions.replace_with_summary(id, summary);
    Ok(())
}

#[cfg(test)]
mod tests;
