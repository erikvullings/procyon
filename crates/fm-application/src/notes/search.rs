//! Finding notes (task 0337). When the notes folder is in the semantic library the index is
//! asked first; otherwise, or when it is unavailable or finds nothing, a bounded text search runs
//! in-process over the folder. No shell command is started, so it works the same on every OS,
//! over every provider and in both hosts.

use async_trait::async_trait;
use fm_agent::NoteKind;
use fm_domain::Location;
use tokio_util::sync::CancellationToken;

use super::format::{NoteMeta, note_title, split_note};
use super::store::NotesStore;

/// Most hits one search returns.
pub(crate) const MAX_HITS: usize = 20;
/// Hits returned when the caller names no limit.
pub(crate) const DEFAULT_HITS: usize = 8;
/// Longest snippet, in characters.
const MAX_SNIPPET_CHARS: usize = 200;
/// Bytes of each note the text search reads.
const MAX_SEARCHED_NOTE_BYTES: u64 = 256 * 1024;
/// Most query terms used.
const MAX_TERMS: usize = 12;

/// Which search produced the hits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SearchStrategy {
    Semantic,
    Text,
}

/// One matching note.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct NoteHit {
    pub(crate) location: Location,
    pub(crate) path: String,
    pub(crate) title: String,
    pub(crate) tags: Vec<String>,
    pub(crate) snippet: String,
    pub(crate) score: f32,
}

/// One passage the semantic index ranked, best first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SemanticPassage {
    pub(crate) location: Location,
    pub(crate) excerpt: String,
}

/// The semantic index could not answer; the text search takes over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SemanticUnavailable;

/// The semantic index of the notes folder, scoped to it and to the person's consent.
#[async_trait]
pub(crate) trait SemanticNotes: Send + Sync {
    async fn search(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SemanticPassage>, SemanticUnavailable>;
}

/// A note's score with what is shown for it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ScoredNote {
    pub(crate) score: f32,
    pub(crate) title: String,
    pub(crate) tags: Vec<String>,
    pub(crate) snippet: String,
}

/// Words too common to tell notes apart.
const FILLER: &[&str] = &[
    "a", "an", "and", "are", "can", "did", "do", "does", "for", "how", "i", "in", "is", "it", "me",
    "my", "of", "on", "or", "should", "the", "to", "was", "what", "when", "where", "which", "with",
    "you",
];

/// Score weights: a title or file-name hit, a tag, a heading, each body occurrence.
const TITLE_WEIGHT: f32 = 10.0;
const TAG_WEIGHT: f32 = 8.0;
const HEADING_WEIGHT: f32 = 4.0;
const BODY_WEIGHT: f32 = 1.0;
/// Most body occurrences of one term that count.
const MAX_BODY_COUNT: usize = 5;

/// The lowercase words of a query, two characters or longer, without repeats or filler words.
pub(crate) fn query_terms(query: &str) -> Vec<String> {
    let mut terms: Vec<String> = Vec::new();
    for word in query
        .split(|c: char| !c.is_alphanumeric())
        .map(str::to_lowercase)
    {
        if word.chars().count() >= 2 && !FILLER.contains(&word.as_str()) && !terms.contains(&word) {
            terms.push(word);
            if terms.len() == MAX_TERMS {
                break;
            }
        }
    }
    terms
}

/// How well one note matches, or `None` when no term occurs in it. Every term scores its best
/// place (title or file name, tag, heading) plus bounded body occurrences; the sum is weighted by
/// the share of terms found, so a note with all terms beats one repeating a single term.
pub(crate) fn score_note(path: &str, text: &str, terms: &[String]) -> Option<ScoredNote> {
    if terms.is_empty() {
        return None;
    }
    let (meta, body) = split_note(text);
    let stem = file_stem(path);
    let title = note_title(meta.as_ref(), body, &stem);
    let tags = meta
        .as_ref()
        .map(|meta| meta.tags.clone())
        .unwrap_or_default();
    let title_text = format!("{} {}", title.to_lowercase(), stem.to_lowercase());
    let tag_text: Vec<String> = tags.iter().map(|tag| tag.to_lowercase()).collect();
    let headings: Vec<String> = body
        .lines()
        .filter(|line| line.trim_start().starts_with('#'))
        .map(str::to_lowercase)
        .collect();
    let lower_body = body.to_lowercase();

    let mut total = 0.0;
    let mut found = 0usize;
    for term in terms {
        let place = if title_text.contains(term.as_str()) {
            TITLE_WEIGHT
        } else if tag_text.iter().any(|tag| tag.contains(term.as_str())) {
            TAG_WEIGHT
        } else if headings
            .iter()
            .any(|heading| heading.contains(term.as_str()))
        {
            HEADING_WEIGHT
        } else {
            0.0
        };
        let occurrences = lower_body
            .matches(term.as_str())
            .count()
            .min(MAX_BODY_COUNT);
        let term_score = place + BODY_WEIGHT * occurrences as f32;
        if term_score > 0.0 {
            found += 1;
            total += term_score;
        }
    }
    if found == 0 {
        return None;
    }
    let coverage = found as f32 / terms.len() as f32;
    Some(ScoredNote {
        score: total * coverage * coverage,
        title,
        tags,
        snippet: snippet(body, terms),
    })
}

/// The first body line holding a term, else the first plain body line, bounded.
fn snippet(body: &str, terms: &[String]) -> String {
    let plain = |line: &&str| {
        let trimmed = line.trim();
        !trimmed.is_empty() && !trimmed.starts_with('#') && !trimmed.starts_with("```")
    };
    let line = body
        .lines()
        .filter(plain)
        .find(|line| {
            let lower = line.to_lowercase();
            terms.iter().any(|term| lower.contains(term.as_str()))
        })
        .or_else(|| body.lines().find(plain))
        .unwrap_or_default();
    bounded(line.trim())
}

fn bounded(text: &str) -> String {
    if text.chars().count() <= MAX_SNIPPET_CHARS {
        return text.to_owned();
    }
    let mut cut: String = text.chars().take(MAX_SNIPPET_CHARS).collect();
    cut.push('…');
    cut
}

fn file_stem(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rsplit_once('.')
        .map_or(name, |(stem, _)| stem)
        .to_owned()
}

/// Searches notes: semantic first when an index is given, text otherwise or as fallback.
pub(crate) async fn search_notes(
    store: &NotesStore,
    semantic: Option<&dyn SemanticNotes>,
    query: &str,
    limit: usize,
    cancellation: &CancellationToken,
) -> (SearchStrategy, Vec<NoteHit>) {
    search_notes_of_kind(store, semantic, query, limit, None, cancellation).await
}

/// The folder that holds the records of assistant sessions (task 0358).
pub(crate) const SESSIONS_FOLDER: &str = "sessions";

/// Whether the note at `path` is the record of an assistant session.
pub(crate) fn is_session_record(path: &str) -> bool {
    path.strip_prefix(SESSIONS_FOLDER)
        .is_some_and(|rest| rest.starts_with('/'))
}

fn of_kind(path: &str, kind: Option<NoteKind>) -> bool {
    match kind {
        None => true,
        Some(NoteKind::Session) => is_session_record(path),
        Some(NoteKind::Note) => !is_session_record(path),
    }
}

/// Like [`search_notes`], keeping only notes or only session records when `kind` says so; the
/// filter applies before the limit, so a narrow search still fills its hits.
pub(crate) async fn search_notes_of_kind(
    store: &NotesStore,
    semantic: Option<&dyn SemanticNotes>,
    query: &str,
    limit: usize,
    kind: Option<NoteKind>,
    cancellation: &CancellationToken,
) -> (SearchStrategy, Vec<NoteHit>) {
    let limit = limit.clamp(1, MAX_HITS);
    if let Some(index) = semantic {
        let hits = semantic_hits(store, index, query, limit, kind).await;
        if !hits.is_empty() {
            return (SearchStrategy::Semantic, hits);
        }
    }
    (
        SearchStrategy::Text,
        text_hits(store, query, limit, kind, cancellation).await,
    )
}

/// The index's passages as notes: one hit per note, best passage first, only inside the folder.
async fn semantic_hits(
    store: &NotesStore,
    index: &dyn SemanticNotes,
    query: &str,
    limit: usize,
    kind: Option<NoteKind>,
) -> Vec<NoteHit> {
    // Several passages may come from one note, so ask for more passages than notes are wanted.
    let Ok(passages) = index.search(query, MAX_HITS.max(limit * 3)).await else {
        return Vec::new();
    };
    let mut hits: Vec<NoteHit> = Vec::new();
    for passage in passages {
        if hits.len() >= limit {
            break;
        }
        let Some(path) = store.path_of(&passage.location).await else {
            continue;
        };
        if !of_kind(&path, kind) || hits.iter().any(|hit| hit.path == path) {
            continue;
        }
        let (title, tags) = describe(store, &passage.location, &path).await;
        hits.push(NoteHit {
            location: passage.location,
            path,
            title,
            tags,
            snippet: bounded(passage.excerpt.trim()),
            score: 0.0,
        });
    }
    let count = hits.len() as f32;
    for (rank, hit) in hits.iter_mut().enumerate() {
        hit.score = (count - rank as f32) / count;
    }
    hits
}

/// A note's title and tags, read from its front matter.
async fn describe(store: &NotesStore, location: &Location, path: &str) -> (String, Vec<String>) {
    let stem = file_stem(path);
    match store
        .read_text(location, MAX_SEARCHED_NOTE_BYTES, path)
        .await
    {
        Ok(text) => {
            let (meta, body) = split_note(&text);
            let tags = meta.as_ref().map(|meta: &NoteMeta| meta.tags.clone());
            (
                note_title(meta.as_ref(), body, &stem),
                tags.unwrap_or_default(),
            )
        }
        Err(_) => (stem, Vec::new()),
    }
}

async fn text_hits(
    store: &NotesStore,
    query: &str,
    limit: usize,
    kind: Option<NoteKind>,
    cancellation: &CancellationToken,
) -> Vec<NoteHit> {
    let terms = query_terms(query);
    if terms.is_empty() {
        return Vec::new();
    }
    let mut hits = Vec::new();
    for file in store.walk(cancellation).await {
        if cancellation.is_cancelled() {
            break;
        }
        if file.size > MAX_SEARCHED_NOTE_BYTES || !of_kind(&file.path, kind) {
            continue;
        }
        let Ok(text) = store
            .read_text(&file.location, MAX_SEARCHED_NOTE_BYTES, &file.path)
            .await
        else {
            continue;
        };
        if let Some(scored) = score_note(&file.path, &text, &terms) {
            hits.push(NoteHit {
                location: file.location,
                path: file.path,
                title: scored.title,
                tags: scored.tags,
                snippet: scored.snippet,
                score: scored.score,
            });
        }
    }
    hits.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.path.cmp(&right.path))
    });
    hits.truncate(limit);
    hits
}

#[cfg(test)]
mod tests;
