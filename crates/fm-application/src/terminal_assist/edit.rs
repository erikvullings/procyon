//! The terminal host's side of `edit_file` (task 0352): reads the file inside the working folder,
//! applies the edit in memory, and shows the person a diff. The file is written only after they
//! approve (or in auto-approve mode), and only when it has not changed since the diff was made.
//!
//! Writes go through the VFS provider like `write_file`'s, so a local and an SSH folder behave the
//! same. An edit is not an operation-engine job: like `write_file` it is one bounded write of a
//! text file the person approved, and the diff they saw is its record.

use std::hash::{DefaultHasher, Hash as _, Hasher as _};

use fm_agent::{EditMatch, EditRequest, MAX_EDITED_FILE_BYTES, match_edit};
use fm_domain::{EntryId, Location};
use fm_vfs::{EntryRef, ProviderRegistry, WriteOptions};
use similar::{ChangeTag, TextDiff};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio_util::sync::CancellationToken;

use crate::agent_host::{ToolHost, is_document};

/// Most diff text shown to the person; a longer diff is cut with a note.
const MAX_DIFF_BYTES: usize = 24 * 1024;

/// An edit worked out and waiting for the person, with what it was made from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingEdit {
    /// The path as the model named it.
    pub(crate) path: String,
    location: Location,
    /// Fingerprint of the text the edit was made from.
    before: u64,
    after: String,
    tier: EditMatch,
    /// Unified diff for the person.
    pub(crate) diff: String,
    pub(crate) added: usize,
    pub(crate) removed: usize,
}

impl PendingEdit {
    /// The line for the person once it is written.
    pub(crate) fn step(&self) -> String {
        format!("Edited {} (+{} −{})", self.path, self.added, self.removed)
    }

    /// The text for the model once it is written.
    pub(crate) fn result(&self) -> String {
        format!(
            "Edited {}: {} line(s) added, {} removed (matched {}).",
            self.path, self.added, self.removed, self.tier
        )
    }
}

fn fingerprint(text: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

/// Works out `request` against the file as it is now. The error is the text for the model.
pub(crate) async fn prepare(
    host: Option<&ToolHost>,
    providers: &ProviderRegistry,
    request: &EditRequest,
) -> Result<PendingEdit, String> {
    let path = request.path.as_str();
    if is_document(path) {
        return Err(format!(
            "{path} is a document the assistant reads by conversion; it cannot be edited."
        ));
    }
    let host = host.ok_or_else(|| "the working folder cannot be reached.".to_owned())?;
    let location = host
        .document_location(providers, path)
        .await
        .map_err(|error| error.to_string())?;
    let before = read_text(providers, &location, path).await?;
    let (after, tier) = match_edit(&before, request).map_err(|error| error.to_string())?;
    let (diff, added, removed) = diff(path, &before, &after);
    Ok(PendingEdit {
        path: path.to_owned(),
        location,
        before: fingerprint(&before),
        after,
        tier,
        diff,
        added,
        removed,
    })
}

/// Writes an approved edit, unless the file changed since it was worked out.
pub(crate) async fn apply(providers: &ProviderRegistry, edit: &PendingEdit) -> Result<(), String> {
    let now = read_text(providers, &edit.location, &edit.path).await?;
    if fingerprint(&now) != edit.before {
        return Err(format!(
            "{} changed after the edit was proposed, so it was not written. Read it again.",
            edit.path
        ));
    }
    let failed = || format!("{} could not be written.", edit.path);
    let provider = providers.resolve(&edit.location).map_err(|_| failed())?;
    let mut stream = provider
        .open_write(
            &edit.location,
            WriteOptions { overwrite: true },
            CancellationToken::new(),
        )
        .await
        .map_err(|_| failed())?;
    stream
        .write_all(edit.after.as_bytes())
        .await
        .map_err(|_| failed())?;
    stream.shutdown().await.map_err(|_| failed())
}

/// The file's text, refused when it is too large or not UTF-8.
async fn read_text(
    providers: &ProviderRegistry,
    location: &Location,
    path: &str,
) -> Result<String, String> {
    let unreadable = || format!("{path} cannot be read.");
    let provider = providers.resolve(location).map_err(|_| unreadable())?;
    let entry = EntryRef {
        id: EntryId::new(),
        location: location.clone(),
    };
    let reader = provider
        .open_read(&entry, CancellationToken::new())
        .await
        .map_err(|_| unreadable())?;
    let mut bytes = Vec::new();
    reader
        .take(MAX_EDITED_FILE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| unreadable())?;
    if bytes.len() > MAX_EDITED_FILE_BYTES {
        return Err(format!(
            "{path} is larger than {} KiB and is not edited.",
            MAX_EDITED_FILE_BYTES / 1024
        ));
    }
    String::from_utf8(bytes).map_err(|_| format!("{path} is not a text file."))
}

/// A unified diff with three lines of context, and the lines added and removed.
fn diff(path: &str, before: &str, after: &str) -> (String, usize, usize) {
    let changes = TextDiff::from_lines(before, after);
    let (mut added, mut removed) = (0, 0);
    for change in changes.iter_all_changes() {
        match change.tag() {
            ChangeTag::Insert => added += 1,
            ChangeTag::Delete => removed += 1,
            ChangeTag::Equal => {}
        }
    }
    let mut text = changes
        .unified_diff()
        .context_radius(3)
        .header(path, path)
        .to_string();
    if text.len() > MAX_DIFF_BYTES {
        text = format!(
            "{}\n… (the rest of the diff is not shown)\n",
            fm_agent::truncate_to_bytes(&text, MAX_DIFF_BYTES)
        );
    }
    (text, added, removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_diff_counts_lines_and_keeps_context() {
        let (text, added, removed) = diff("a.md", "one\ntwo\nthree\n", "one\nTWO\nthree\nfour\n");
        assert_eq!((added, removed), (2, 1));
        assert!(text.starts_with("--- a.md\n+++ a.md\n"), "{text}");
        assert!(text.contains("-two\n+TWO\n"), "{text}");
        assert!(text.contains(" one\n"), "{text}");
    }
}
