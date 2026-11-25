//! Saved terminal assistant sessions (task 0354): each session with at least one question is kept
//! as one JSON document under the settings data root, so it survives the tab and the application
//! and can be resumed in any tab (`/resume`). Resuming copies the saved conversation into the
//! tab's own session, so the original stays as it was: a resumed session is a fork.
//!
//! What is stored is what the model saw: questions with their attached context, replies, tool
//! calls and results, already redacted and bounded per turn. It never leaves the machine.

use std::fs;
use std::path::{Path, PathBuf};

use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};

use super::session::Turn;

/// Directory under the settings data root.
pub(crate) const STORE_DIRECTORY: &str = "terminal-sessions";
/// Schema of one saved session written by this build.
const SCHEMA_VERSION: u32 = 1;
/// Most sessions kept; the least recently changed are removed beyond this.
pub(crate) const MAX_SAVED_SESSIONS: usize = 50;
/// Longest title, in characters.
const TITLE_CHARS: usize = 80;

/// One saved session.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SavedSession {
    schema: u32,
    pub(super) id: String,
    pub(super) title: String,
    /// The session's folder, as a location URI.
    pub(super) folder: String,
    pub(super) updated_at: String,
    pub(super) questions: usize,
    pub(super) turns: Vec<Turn>,
}

/// A saved session without its conversation, for the `/resume` list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SavedSummary {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) folder: String,
    pub(crate) updated_at: String,
    pub(crate) questions: usize,
}

/// The saved sessions in one directory.
#[derive(Debug, Clone)]
pub(crate) struct SavedSessions {
    root: PathBuf,
}

impl SavedSessions {
    pub(crate) fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// Saves a session's conversation under `id`; a session without a question is not saved.
    /// Failures are ignored: a session that cannot be saved still works in its tab.
    pub(super) fn save(&self, id: &str, folder: &str, title: &str, turns: &[Turn]) {
        let questions = turns.iter().filter(|turn| turn.is_person()).count();
        if questions == 0 {
            return;
        }
        let record = SavedSession {
            schema: SCHEMA_VERSION,
            id: id.to_owned(),
            title: title.chars().take(TITLE_CHARS).collect(),
            folder: folder.to_owned(),
            updated_at: Utc::now().to_rfc3339_opts(SecondsFormat::Nanos, true),
            questions,
            turns: turns.to_vec(),
        };
        let Ok(text) = serde_json::to_vec(&record) else {
            return;
        };
        if fs::create_dir_all(&self.root).is_err() {
            return;
        }
        let path = self.path(id);
        let temporary = path.with_extension("json.tmp");
        if fs::write(&temporary, text).is_ok() && fs::rename(&temporary, &path).is_ok() {
            self.prune();
        }
    }

    /// The saved sessions, most recently changed first.
    pub(crate) fn list(&self) -> Vec<SavedSummary> {
        let mut summaries: Vec<SavedSummary> = self
            .records()
            .into_iter()
            .map(|(_, record)| SavedSummary {
                id: record.id,
                title: record.title,
                folder: record.folder,
                updated_at: record.updated_at,
                questions: record.questions,
            })
            .collect();
        summaries.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        summaries
    }

    /// One saved session.
    pub(super) fn load(&self, id: &str) -> Option<SavedSession> {
        let text = fs::read(self.path(id)).ok()?;
        serde_json::from_slice::<SavedSession>(&text)
            .ok()
            .filter(|record| record.schema == SCHEMA_VERSION && record.id == id)
    }

    fn records(&self) -> Vec<(PathBuf, SavedSession)> {
        let Ok(entries) = fs::read_dir(&self.root) else {
            return Vec::new();
        };
        entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "json")
            })
            .filter_map(|path| {
                let record = serde_json::from_slice::<SavedSession>(&fs::read(&path).ok()?).ok()?;
                (record.schema == SCHEMA_VERSION).then_some((path, record))
            })
            .collect()
    }

    /// Removes the oldest sessions beyond [`MAX_SAVED_SESSIONS`].
    fn prune(&self) {
        let mut records = self.records();
        if records.len() <= MAX_SAVED_SESSIONS {
            return;
        }
        records.sort_by(|(_, a), (_, b)| b.updated_at.cmp(&a.updated_at));
        for (path, _) in records.into_iter().skip(MAX_SAVED_SESSIONS) {
            let _ = fs::remove_file(path);
        }
    }

    /// The file of `id`: its characters that are safe in a file name on every platform.
    fn path(&self, id: &str) -> PathBuf {
        let name: String = id
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        Path::new(&self.root).join(format!("{name}.json"))
    }
}

#[cfg(test)]
mod tests {
    use super::super::session::Sessions;
    use super::*;

    fn sessions(dir: &Path) -> Sessions {
        Sessions::persistent(dir.join(STORE_DIRECTORY))
    }

    #[test]
    fn a_session_survives_a_restart_and_resumes_as_a_copy() {
        let dir = tempfile::tempdir().unwrap();
        let first = sessions(dir.path());
        first.push_person(
            "tab-a",
            "where are the decoy files?",
            "Folder listing: a.pdf",
        );
        first.push_assistant("tab-a", "In ./decoys.", &[]);
        first.save("tab-a", "file:///work");
        // An empty session is not worth saving.
        first.save("tab-empty", "file:///work");
        drop(first);

        let later = sessions(dir.path());
        let saved = later.saved();
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].title, "where are the decoy files?");
        assert_eq!(saved[0].folder, "file:///work");
        assert_eq!(saved[0].questions, 1);

        let resumed = later.resume("tab-a", "tab-b").unwrap();
        assert_eq!(resumed.id, "tab-a");
        let text = later.render("tab-b");
        assert!(text.contains("where are the decoy files?"), "{text}");
        assert!(text.contains("In ./decoys."), "{text}");
        assert_eq!(later.last_question("tab-b"), "where are the decoy files?");

        // Going on in the new tab saves under its own id; the original stays as it was.
        later.push_person("tab-b", "and the PDFs?", "");
        later.save("tab-b", "file:///work");
        assert_eq!(later.saved().len(), 2);
        let original = later.resume("tab-a", "tab-c").unwrap();
        assert_eq!(original.questions, 1);
        assert!(later.resume("missing", "tab-d").is_none());
    }

    #[test]
    fn only_the_newest_sessions_are_kept() {
        let dir = tempfile::tempdir().unwrap();
        let store = sessions(dir.path());
        for index in 0..MAX_SAVED_SESSIONS + 3 {
            let id = format!("tab:{index}");
            store.push_person(&id, &format!("question {index}"), "");
            store.save(&id, "file:///work");
        }
        let saved = store.saved();
        assert_eq!(saved.len(), MAX_SAVED_SESSIONS);
        assert!(saved.iter().all(|summary| summary.title != "question 0"));
        // Ids with characters unsafe in file names still round-trip.
        assert!(
            store
                .resume(&format!("tab:{}", MAX_SAVED_SESSIONS + 2), "x")
                .is_some()
        );
    }
}
