//! Records of assistant sessions in the notes folder (task 0358): one Markdown file per terminal
//! session or chat conversation in `sessions/`, updated in place, so the folder's existing
//! semantic consent and indexing make earlier sessions searchable with `search_notes`. The task
//! asks for this narrow writer next to `NotesService` (which never writes notes): it writes
//! through the VFS provider, nothing but `sessions/*.md`, and when pruning removes only records it
//! wrote itself, to the trash.
//!
//! A record holds a summary of the session, the questions asked and the paths of files the tools
//! touched; never file contents or command output. Everything is redacted before it is written.

use chrono::{DateTime, SecondsFormat, Utc};
use fm_agent::redact_secrets;
use fm_domain::{EntryId, Location};
use fm_vfs::{EntryRef, ProviderRegistry, RemoveOptions, WriteOptions};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt as _;
use tokio_util::sync::CancellationToken;

use super::format::{NoteMeta, render_note, slugify, split_note};
use super::search::SESSIONS_FOLDER;
use super::store::{NotesError, NotesStore};
use crate::file_assistant::truncate_to_bytes;

/// Most session records kept; the least recently updated are removed beyond this.
pub(crate) const MAX_SESSION_RECORDS: usize = 200;
/// Most questions listed in a record (the newest).
const MAX_QUESTIONS: usize = 20;
/// Most file paths listed in a record.
const MAX_FILES: usize = 30;
/// Longest question line, in characters.
const MAX_QUESTION_CHARS: usize = 200;
/// Longest summary, in bytes.
const MAX_SUMMARY_BYTES: usize = 16 * 1024;
/// Longest title, in characters.
const MAX_TITLE_CHARS: usize = 80;
/// The `source` of records this writer made; only those are ever pruned.
const RECORD_SOURCE: &str = "procyon-assistant";
/// The tools whose `path` argument is a file a session read or changed.
const FILE_TOOLS: &[&str] = &["read_file", "summarize_file", "edit_file", "write_file"];

/// The files the calls of a reply read or changed: the `path` of the file tools, paths only. Both
/// assistants record files this way.
pub(crate) fn touched_paths(calls: &[fm_agent::ToolCallRecord]) -> impl Iterator<Item = &str> {
    calls
        .iter()
        .filter(|call| FILE_TOOLS.contains(&call.name.as_str()))
        .filter_map(|call| call.arguments.get("path").and_then(|path| path.as_str()))
}

/// Which assistant a session belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionHost {
    Terminal,
    Chat,
}

impl SessionHost {
    fn name(self) -> &'static str {
        match self {
            Self::Terminal => "terminal",
            Self::Chat => "chat",
        }
    }
}

/// What a record says about one session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SessionRecord {
    /// The session's stable id (a terminal tab session, a chat conversation); the record is found
    /// again by it, so it is updated rather than duplicated.
    pub(crate) id: String,
    pub(crate) host: SessionHost,
    /// The first question, or the conversation's title.
    pub(crate) title: String,
    /// The folder the session worked in, as a location URI.
    pub(crate) folder: Option<String>,
    /// When the session started, RFC 3339; kept from the first record when there is one.
    pub(crate) started: Option<String>,
    pub(crate) summary: String,
    pub(crate) questions: Vec<String>,
    /// Paths the tools read or changed (paths only).
    pub(crate) files: Vec<String>,
}

/// Writes session records into one notes folder.
pub(crate) struct SessionRecords {
    providers: ProviderRegistry,
    root: Location,
    /// Most records kept.
    limit: usize,
    /// Whether pruned records go to the trash (tests turn it off, not to fill the real trash).
    use_trash: bool,
}

impl SessionRecords {
    pub(crate) fn new(providers: ProviderRegistry, root: Location) -> Self {
        Self {
            providers,
            root,
            limit: MAX_SESSION_RECORDS,
            use_trash: true,
        }
    }

    /// A writer keeping at most `limit` records and deleting pruned ones outright.
    #[cfg(test)]
    fn keeping(mut self, limit: usize) -> Self {
        self.limit = limit;
        self.use_trash = false;
        self
    }

    /// The writer, when the person turned session records on and set a notes folder; `None`
    /// otherwise, so nothing is written.
    pub(crate) fn for_settings(
        settings: &fm_settings::Settings,
        providers: &ProviderRegistry,
    ) -> Option<Self> {
        if !settings.assistant_session_memory {
            return None;
        }
        let root = settings.notes_root.clone()?;
        Some(Self::new(providers.clone(), root))
    }

    /// Writes (or rewrites) the record of `record.id` and returns its path in the notes folder.
    pub(crate) async fn write(
        &self,
        record: &SessionRecord,
        now: DateTime<Utc>,
    ) -> Result<String, NotesError> {
        let store = NotesStore::open(&self.providers, &self.root).await?;
        let provider = self
            .providers
            .resolve(&self.root)
            .map_err(|_| NotesError::RootUnavailable)?;
        let unavailable = || NotesError::NotAvailable(SESSIONS_FOLDER.to_owned());
        let folder = match store.folder(SESSIONS_FOLDER).await {
            Some(folder) => folder,
            None => provider
                .create_directory(&self.root, SESSIONS_FOLDER, CancellationToken::new())
                .await
                .map(|entry| entry.location)
                .map_err(|_| unavailable())?,
        };
        let tag = id_tag(&record.id);
        // Only `sessions/` is listed, never the rest of the notes folder.
        let existing = store
            .notes_in(SESSIONS_FOLDER)
            .await
            .into_iter()
            .find(|file| file.path.ends_with(&format!("-{tag}.md")));
        let earlier = match &existing {
            Some(file) => store
                .read(&file.path)
                .await
                .ok()
                .and_then(|note| split_note(&note.text).0),
            None => None,
        };
        let stamp = now.to_rfc3339_opts(SecondsFormat::Secs, true);
        let started = earlier
            .as_ref()
            .and_then(|meta| meta.started.clone())
            .or_else(|| record.started.clone())
            .unwrap_or_else(|| stamp.clone());
        let title = one_line(&record.title, MAX_TITLE_CHARS);
        let title = if title.is_empty() {
            "Assistant session".to_owned()
        } else {
            title
        };
        let (location, path) = match existing {
            Some(file) => (file.location, file.path),
            None => {
                let date = started.get(..10).unwrap_or(&stamp[..10]);
                let name = format!("{date}-{}-{tag}.md", slugify(&title));
                let location = folder.join(&name).map_err(|_| unavailable())?;
                (location, format!("{SESSIONS_FOLDER}/{name}"))
            }
        };
        let meta = NoteMeta {
            title: Some(title.clone()),
            started: Some(started),
            updated: Some(stamp),
            tags: vec!["assistant-session".to_owned()],
            source: Some(RECORD_SOURCE.to_owned()),
            kind: Some("session".to_owned()),
            host: Some(record.host.name().to_owned()),
            folder: record.folder.clone(),
            ..NoteMeta::default()
        };
        let content = redact_secrets(&render_note(&meta, &body(&title, record)));
        let mut stream = provider
            .open_write(
                &location,
                WriteOptions { overwrite: true },
                CancellationToken::new(),
            )
            .await
            .map_err(|_| NotesError::NotAvailable(path.clone()))?;
        stream
            .write_all(content.as_bytes())
            .await
            .map_err(|_| NotesError::NotAvailable(path.clone()))?;
        stream
            .shutdown()
            .await
            .map_err(|_| NotesError::NotAvailable(path.clone()))?;
        self.prune(&store).await;
        Ok(path)
    }

    /// Removes the least recently updated records beyond the limit: only records
    /// this writer made (`kind: session`, `source: procyon-assistant`), to the trash. Anything else
    /// in `sessions/` is the person's and is left alone.
    async fn prune(&self, store: &NotesStore) {
        let files = store.notes_in(SESSIONS_FOLDER).await;
        if files.len() <= self.limit {
            return;
        }
        let mut ours = Vec::with_capacity(files.len());
        for file in files {
            let Ok(note) = store.read(&file.path).await else {
                continue;
            };
            if let Some(meta) = split_note(&note.text).0
                && meta.kind.as_deref() == Some("session")
                && meta.source.as_deref() == Some(RECORD_SOURCE)
            {
                ours.push((meta.updated.unwrap_or_default(), file));
            }
        }
        if ours.len() <= self.limit {
            return;
        }
        ours.sort_by(|(left, _), (right, _)| right.cmp(left));
        let Ok(provider) = self.providers.resolve(&self.root) else {
            return;
        };
        for (_, file) in ours.into_iter().skip(self.limit) {
            let entry = EntryRef {
                id: EntryId::new(),
                location: file.location,
            };
            let _ = provider
                .remove(
                    &entry,
                    RemoveOptions {
                        recursive: false,
                        use_trash: self.use_trash,
                    },
                    CancellationToken::new(),
                )
                .await;
        }
    }
}

/// A short, stable, file-name-safe tag for a session id.
fn id_tag(id: &str) -> String {
    Sha256::digest(id.as_bytes())
        .iter()
        .take(4)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// `text` on one line, at most `max` characters.
fn one_line(text: &str, max: usize) -> String {
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.chars().count() > max {
        format!("{}…", line.chars().take(max - 1).collect::<String>())
    } else {
        line
    }
}

fn body(title: &str, record: &SessionRecord) -> String {
    let mut body = format!(
        "# {title}\n\n{}\n",
        truncate_to_bytes(record.summary.trim(), MAX_SUMMARY_BYTES)
    );
    if !record.questions.is_empty() {
        body.push_str("\n## Questions\n\n");
        let skip = record.questions.len().saturating_sub(MAX_QUESTIONS);
        // The first line only: text pasted below a question (output, a file) stays out.
        for question in record.questions.iter().skip(skip) {
            let first = question
                .lines()
                .find(|line| !line.trim().is_empty())
                .unwrap_or("");
            body.push_str(&format!("- {}\n", one_line(first, MAX_QUESTION_CHARS)));
        }
    }
    if !record.files.is_empty() {
        body.push_str("\n## Files\n\n");
        for file in record.files.iter().take(MAX_FILES) {
            body.push_str(&format!("- `{}`\n", one_line(file, MAX_QUESTION_CHARS)));
        }
    }
    body
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone as _;

    use super::*;
    use crate::notes::store::tests::{location_of, registry};

    fn providers() -> ProviderRegistry {
        registry()
    }

    fn record(id: &str, summary: &str) -> SessionRecord {
        SessionRecord {
            id: id.into(),
            host: SessionHost::Terminal,
            title: "Where are the decoy files?".into(),
            folder: Some("file:///work".into()),
            started: Some("2026-11-20T09:00:00Z".into()),
            summary: summary.into(),
            questions: vec!["where are the decoy files?".into()],
            files: vec!["decoys/a.pdf".into()],
        }
    }

    fn at(day: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, day, 10, 0, 0).unwrap()
    }

    #[tokio::test]
    async fn a_record_is_written_with_front_matter_and_updated_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let root = location_of(dir.path());
        let records = SessionRecords::new(providers(), root);
        let path = records
            .write(&record("tab-1", "They are in ./decoys."), at(20))
            .await
            .unwrap();
        assert!(path.starts_with("sessions/2026-11-20-"), "{path}");
        let text = std::fs::read_to_string(dir.path().join(&path)).unwrap();
        let (meta, body) = split_note(&text);
        let meta = meta.unwrap();
        assert_eq!(meta.kind.as_deref(), Some("session"));
        assert_eq!(meta.host.as_deref(), Some("terminal"));
        assert_eq!(meta.folder.as_deref(), Some("file:///work"));
        assert_eq!(meta.started.as_deref(), Some("2026-11-20T09:00:00Z"));
        assert!(body.contains("They are in ./decoys."));
        assert!(body.contains("- where are the decoy files?"));
        assert!(body.contains("- `decoys/a.pdf`"));

        // The same session again: the same file, a new summary, the start kept.
        let again = records
            .write(
                &record("tab-1", "They are in ./decoys; two were PDFs."),
                at(21),
            )
            .await
            .unwrap();
        assert_eq!(again, path);
        let files: Vec<_> = std::fs::read_dir(dir.path().join("sessions"))
            .unwrap()
            .collect();
        assert_eq!(files.len(), 1);
        let text = std::fs::read_to_string(dir.path().join(&path)).unwrap();
        let meta = split_note(&text).0.unwrap();
        assert_eq!(meta.started.as_deref(), Some("2026-11-20T09:00:00Z"));
        assert_eq!(meta.updated.as_deref(), Some("2026-11-21T10:00:00Z"));
        assert!(text.contains("two were PDFs"));
    }

    #[tokio::test]
    async fn only_the_newest_own_records_are_kept_and_the_persons_files_stay() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("sessions")).unwrap();
        std::fs::write(dir.path().join("sessions/my-own-notes.md"), "# Mine\n").unwrap();
        let records = SessionRecords::new(providers(), location_of(dir.path())).keeping(2);
        for (index, day) in [(1, 20), (2, 21), (3, 22)] {
            records
                .write(&record(&format!("tab-{index}"), "x"), at(day))
                .await
                .unwrap();
        }
        let mut names: Vec<String> = std::fs::read_dir(dir.path().join("sessions"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names.len(), 3, "{names:?}");
        assert!(names.contains(&"my-own-notes.md".to_owned()));
        // The oldest record (tab-1, updated on the 20th) was removed.
        let tag = id_tag("tab-1");
        assert!(
            !names
                .iter()
                .any(|name| name.ends_with(&format!("-{tag}.md")))
        );
    }

    #[tokio::test]
    async fn a_question_keeps_its_first_line_only() {
        let dir = tempfile::tempdir().unwrap();
        let records = SessionRecords::new(providers(), location_of(dir.path()));
        let mut pasted = record("tab-4", "Explained the error.");
        pasted.questions =
            vec!["why does this fail?\nTraceback (most recent call last):\n  secret output".into()];
        let path = records.write(&pasted, at(20)).await.unwrap();
        let text = std::fs::read_to_string(dir.path().join(path)).unwrap();
        assert!(text.contains("- why does this fail?\n"), "{text}");
        assert!(!text.contains("Traceback"), "{text}");
    }

    #[tokio::test]
    async fn secrets_are_redacted_before_writing() {
        let dir = tempfile::tempdir().unwrap();
        let root = location_of(dir.path());
        let records = SessionRecords::new(providers(), root);
        let mut leaky = record(
            "tab-2",
            "Set API_KEY=sk-live-abcdefghijklmnopqrstuvwxyz0123 and ran it.",
        );
        leaky.questions = vec!["use password=hunter2hunter2 please".into()];
        let path = records.write(&leaky, at(20)).await.unwrap();
        let text = std::fs::read_to_string(dir.path().join(path)).unwrap();
        assert!(
            !text.contains("sk-live-abcdefghijklmnopqrstuvwxyz0123"),
            "{text}"
        );
        assert!(!text.contains("hunter2hunter2"), "{text}");
        assert!(text.contains("[redacted"), "{text}");
    }

    #[test]
    fn records_are_written_only_when_turned_on_with_a_notes_folder() {
        let dir = tempfile::tempdir().unwrap();
        let mut settings = fm_settings::Settings::default();
        assert!(!settings.assistant_session_memory, "off by default");
        settings.notes_root = Some(location_of(dir.path()));
        assert!(SessionRecords::for_settings(&settings, &providers()).is_none());
        settings.assistant_session_memory = true;
        assert!(SessionRecords::for_settings(&settings, &providers()).is_some());
        settings.notes_root = None;
        assert!(SessionRecords::for_settings(&settings, &providers()).is_none());
    }

    #[tokio::test]
    async fn without_a_notes_folder_nothing_is_written() {
        let dir = tempfile::tempdir().unwrap();
        let missing = Location::new(
            location_of(dir.path()).provider_id,
            url::Url::from_directory_path(dir.path().canonicalize().unwrap().join("missing"))
                .unwrap()
                .to_string(),
        );
        let records = SessionRecords::new(providers(), missing);
        assert!(records.write(&record("tab-3", "x"), at(20)).await.is_err());
        assert!(!dir.path().join("missing").exists());
    }
}
