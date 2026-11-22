//! The notes folder as the terminal assistant uses it (task 0337): search and read without
//! approval, inside the notes folder only; proposals come back for the person to review. Text
//! given to the model and proposed notes are redacted.

use async_trait::async_trait;
use chrono::{DateTime, SecondsFormat, Utc};
use fm_transport_dto::{NoteDraftDto, NoteProposalDto};
use tokio_util::sync::CancellationToken;

use super::format::{NoteMeta, note_title, render_note, slugify, split_note};
use super::search::{DEFAULT_HITS, SearchStrategy, SemanticNotes, search_notes};
use super::store::NotesStore;
use crate::file_assistant::truncate_to_bytes;
use crate::terminal_assist::{NoteToolCall, NoteToolOutcome, NotesAccess, redact_secrets};
use fm_agent::NoteSkill;

/// Most bytes of one note shown to the model.
const MAX_NOTE_TEXT_BYTES: usize = 16 * 1024;
/// Largest proposed note, in bytes.
const MAX_PROPOSAL_BYTES: usize = 64 * 1024;
/// Most notes offered as skills for one request.
const NOTE_SKILLS: usize = 5;
/// What the redaction marks start with.
const REDACTION_MARK: &str = "[redacted";

/// The notes folder for one assistant request; `store` is `None` when notes are not set up.
pub(crate) struct AssistantNotes<'a> {
    pub(crate) store: Option<NotesStore>,
    pub(crate) semantic: Option<Box<dyn SemanticNotes + 'a>>,
}

#[async_trait]
impl NotesAccess for AssistantNotes<'_> {
    async fn skills(&self, query: &str) -> Vec<NoteSkill> {
        match &self.store {
            Some(store) => self.note_skills(store, query).await,
            None => Vec::new(),
        }
    }

    async fn run(&self, call: NoteToolCall) -> NoteToolOutcome {
        let Some(store) = &self.store else {
            return output(
                "Looked in the notes",
                "Error: notes are not set up yet. The person can set them up with Save as note \
                 in the terminal."
                    .to_owned(),
            );
        };
        match call {
            NoteToolCall::Search { query, limit } => self.search(store, &query, limit).await,
            NoteToolCall::Read { path } => match store.read(&path).await {
                Ok(note) => output(
                    &format!("Read note {}", note.path),
                    format!(
                        "Note {}:\n{}",
                        note.path,
                        redact_secrets(truncate_to_bytes(&note.text, MAX_NOTE_TEXT_BYTES))
                    ),
                ),
                Err(error) => output(&format!("Read note {path}"), format!("Error: {error}.")),
            },
            NoteToolCall::Propose {
                path,
                title,
                content,
            } => {
                let path = match path {
                    Some(path) => match store.locate(&path).await {
                        Ok(location) => store.path_of(&location).await.or(Some(path)),
                        Err(error) => {
                            return output(
                                &format!("Looked for note {path}"),
                                format!("Error: {error}. Read or search the notes first."),
                            );
                        }
                    },
                    None => None,
                };
                let draft = proposed_draft(title.as_deref(), &content, path.is_some(), Utc::now());
                NoteToolOutcome::Proposal(NoteProposalDto { path, draft })
            }
        }
    }
}

impl AssistantNotes<'_> {
    /// The notes that best match `query`, as skills: the front matter `name` and `description`,
    /// else a slug of the title and the matching snippet (task 0339).
    async fn note_skills(&self, store: &NotesStore, query: &str) -> Vec<NoteSkill> {
        if query.trim().is_empty() {
            return Vec::new();
        }
        let (_, hits) = search_notes(
            store,
            self.semantic.as_deref(),
            query,
            NOTE_SKILLS,
            &CancellationToken::new(),
        )
        .await;
        let mut skills = Vec::new();
        for hit in hits {
            let meta = match store.read(&hit.path).await {
                Ok(note) => split_note(&note.text).0,
                Err(_) => None,
            };
            let name = meta
                .as_ref()
                .and_then(|meta| meta.name.clone())
                .filter(|name| !name.trim().is_empty() && !name.contains(char::is_whitespace))
                .unwrap_or_else(|| slugify(&hit.title));
            let description = meta
                .and_then(|meta| meta.description)
                .filter(|text| !text.trim().is_empty())
                .unwrap_or(hit.snippet);
            skills.push(NoteSkill {
                name,
                description: redact_secrets(&description),
                path: hit.path,
            });
        }
        skills
    }

    async fn search(&self, store: &NotesStore, query: &str, limit: Option<u32>) -> NoteToolOutcome {
        let limit = limit.map_or(DEFAULT_HITS, |limit| limit as usize);
        let (strategy, hits) = search_notes(
            store,
            self.semantic.as_deref(),
            query,
            limit,
            &CancellationToken::new(),
        )
        .await;
        let step = format!("Searched notes for {query}");
        if hits.is_empty() {
            return output(&step, "No notes found.".to_owned());
        }
        let how = match strategy {
            SearchStrategy::Semantic => "semantic search",
            SearchStrategy::Text => "text search",
        };
        let mut text = format!("Notes found by {how}:");
        for (number, hit) in hits.iter().enumerate() {
            let tags = if hit.tags.is_empty() {
                String::new()
            } else {
                format!(" [{}]", hit.tags.join(", "))
            };
            text.push_str(&format!(
                "\n{}. {} — {}{tags}\n   {}",
                number + 1,
                hit.path,
                hit.title,
                hit.snippet
            ));
        }
        output(&step, redact_secrets(&text))
    }
}

fn output(step: &str, text: String) -> NoteToolOutcome {
    NoteToolOutcome::Output {
        step: step.to_owned(),
        text,
    }
}

/// The note the model proposed, as a draft for review. A new note without front matter gets it;
/// a replacement keeps the model's text as written, so keys other tools rely on are not dropped.
fn proposed_draft(
    title: Option<&str>,
    content: &str,
    replacing: bool,
    now: DateTime<Utc>,
) -> NoteDraftDto {
    let content = truncate_to_bytes(content.trim(), MAX_PROPOSAL_BYTES);
    let (meta, body) = split_note(content);
    let title = title
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| note_title(meta.as_ref(), body, "Note"));
    let tags = meta
        .as_ref()
        .map(|meta| meta.tags.clone())
        .unwrap_or_default();
    let full = if meta.is_some() || replacing {
        format!("{content}\n")
    } else {
        let stamp = now.to_rfc3339_opts(SecondsFormat::Secs, true);
        let body = if body.trim_start().starts_with("# ") {
            format!("{}\n", body.trim())
        } else {
            format!("# {title}\n\n{}\n", body.trim())
        };
        render_note(
            &NoteMeta {
                title: Some(title.clone()),
                created: Some(stamp.clone()),
                updated: Some(stamp),
                source: Some("terminal-assistant".to_owned()),
                ..NoteMeta::default()
            },
            &body,
        )
    };
    let content = redact_secrets(&full);
    NoteDraftDto {
        file_name: format!("{}.md", slugify(&title)),
        redactions: u32::try_from(content.matches(REDACTION_MARK).count()).unwrap_or(u32::MAX),
        title,
        tags,
        content,
    }
}

#[cfg(test)]
mod tests;
