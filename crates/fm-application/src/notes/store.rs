//! Read access to the notes folder (task 0337), through its VFS provider so a local folder and
//! one on another machine behave the same. Nothing outside the folder is reachable: `..`,
//! absolute paths, symlinks, hidden and credential folders, and secret-looking file names are all
//! "not available". Everything is bounded. Writing is not done here: new notes go through the
//! operation engine and changes through the editor save.

use std::sync::Arc;

use fm_domain::{EntryId, EntryKind, EntrySummary, Location};
use fm_vfs::{EntryRef, FileSystemProvider, ListOptions, ProviderRegistry};
use thiserror::Error;
use tokio::io::AsyncReadExt as _;
use tokio_util::sync::CancellationToken;

use crate::terminal_assist::{is_credential_folder, is_secret_name};

/// Largest note read, in bytes; the editor's limit.
pub(crate) const MAX_NOTE_BYTES: u64 = crate::file_editor::MAX_EDITABLE_FILE_BYTES;
/// Most note files a walk returns.
pub(crate) const MAX_WALK_FILES: usize = 2000;
/// Deepest folder level a walk enters below the notes folder.
const MAX_WALK_DEPTH: usize = 8;
/// Entries requested per directory page.
const PAGE_SIZE: usize = 256;
/// File extensions treated as notes.
const NOTE_EXTENSIONS: &[&str] = &["md", "markdown", "txt"];

/// Why the notes folder or a note cannot be used.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub(crate) enum NotesError {
    /// Notes have not been set up.
    #[error("notes are not set up")]
    NotConfigured,
    /// No provider serves the notes folder, or it is not a folder.
    #[error("the notes folder is not available")]
    RootUnavailable,
    /// The path is missing, outside the notes folder, or refused. All read the same, so a caller
    /// learns nothing about paths outside the folder.
    #[error("{0} is not available")]
    NotAvailable(String),
    /// The note is not a regular UTF-8 text file within the size limit.
    #[error("{0} is not a readable text note")]
    NotText(String),
}

/// A note file found in the folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NoteFile {
    pub(crate) location: Location,
    /// Path inside the notes folder, with `/` separators.
    pub(crate) path: String,
    pub(crate) size: u64,
}

/// A note's text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StoredNote {
    pub(crate) location: Location,
    pub(crate) path: String,
    pub(crate) text: String,
}

/// The notes folder, reached through its provider.
#[derive(Clone)]
pub(crate) struct NotesStore {
    provider: Arc<dyn FileSystemProvider>,
    root: Location,
}

impl NotesStore {
    /// Opens the folder; it must exist and be a folder (not a symlink to one).
    pub(crate) async fn open(
        providers: &ProviderRegistry,
        root: &Location,
    ) -> Result<Self, NotesError> {
        let provider = providers
            .resolve(root)
            .map_err(|_| NotesError::RootUnavailable)?;
        let store = Self {
            provider,
            root: root.clone(),
        };
        match store.inspect(root).await {
            Some(summary) if summary.kind == EntryKind::Directory => Ok(store),
            _ => Err(NotesError::RootUnavailable),
        }
    }

    /// The notes folder.
    pub(crate) fn root(&self) -> &Location {
        &self.root
    }

    /// The location of a note file inside the folder, checked piece by piece without following
    /// links.
    pub(crate) async fn locate(&self, path: &str) -> Result<Location, NotesError> {
        let unavailable = || NotesError::NotAvailable(path.to_owned());
        let parts = path_parts(path).ok_or_else(unavailable)?;
        let (file_name, folders) = parts.split_last().ok_or_else(unavailable)?;
        if is_secret_name(file_name) || !is_note_name(file_name) {
            return Err(unavailable());
        }
        let mut location = self.root.clone();
        for folder in folders {
            location = location.join(folder).map_err(|_| unavailable())?;
            match self.inspect(&location).await {
                Some(summary) if summary.kind == EntryKind::Directory => {}
                _ => return Err(unavailable()),
            }
        }
        location = location.join(file_name).map_err(|_| unavailable())?;
        match self.inspect(&location).await {
            Some(summary) if summary.kind == EntryKind::File => Ok(location),
            _ => Err(unavailable()),
        }
    }

    /// Reads one note, at most [`MAX_NOTE_BYTES`].
    pub(crate) async fn read(&self, path: &str) -> Result<StoredNote, NotesError> {
        let location = self.locate(path).await?;
        let text = self.read_text(&location, MAX_NOTE_BYTES, path).await?;
        Ok(StoredNote {
            location,
            path: normalised(path),
            text,
        })
    }

    /// Reads at most `limit` bytes of a located note; longer files are refused.
    pub(crate) async fn read_text(
        &self,
        location: &Location,
        limit: u64,
        shown: &str,
    ) -> Result<String, NotesError> {
        let not_text = || NotesError::NotText(shown.to_owned());
        let entry = EntryRef {
            id: EntryId::new(),
            location: location.clone(),
        };
        let stream = self
            .provider
            .open_read(&entry, CancellationToken::new())
            .await
            .map_err(|_| not_text())?;
        let mut buffer = Vec::new();
        stream
            .take(limit + 1)
            .read_to_end(&mut buffer)
            .await
            .map_err(|_| not_text())?;
        if buffer.len() as u64 > limit || fm_vfs::looks_like_binary(&buffer) {
            return Err(not_text());
        }
        String::from_utf8(buffer).map_err(|_| not_text())
    }

    /// The note files in the folder, sorted by path, bounded by [`MAX_WALK_FILES`] and depth.
    pub(crate) async fn walk(&self, cancellation: &CancellationToken) -> Vec<NoteFile> {
        let mut files = Vec::new();
        let mut pending = vec![(self.root.clone(), String::new(), 0usize)];
        while let Some((folder, prefix, depth)) = pending.pop() {
            if cancellation.is_cancelled() || files.len() >= MAX_WALK_FILES {
                break;
            }
            for entry in self.list_all(&folder, cancellation).await {
                let path = format!("{prefix}{}", entry.name);
                match entry.kind {
                    EntryKind::Directory
                        if depth < MAX_WALK_DEPTH && !is_skipped_folder(&entry.name) =>
                    {
                        if let Ok(location) = folder.join(&entry.name) {
                            pending.push((location, format!("{path}/"), depth + 1));
                        }
                    }
                    EntryKind::File
                        if is_note_name(&entry.name) && !is_secret_name(&entry.name) =>
                    {
                        if let Ok(location) = folder.join(&entry.name) {
                            files.push(NoteFile {
                                location,
                                path,
                                size: entry.size.unwrap_or(0),
                            });
                        }
                    }
                    _ => {}
                }
            }
        }
        files.sort_by(|left, right| left.path.cmp(&right.path));
        files.truncate(MAX_WALK_FILES);
        files
    }

    /// The path of `location` inside the folder, when it is inside and reachable without links.
    pub(crate) async fn path_of(&self, location: &Location) -> Option<String> {
        if location.provider_id != self.root.provider_id {
            return None;
        }
        let root = url::Url::parse(&self.root.uri).ok()?;
        let target = url::Url::parse(&location.uri).ok()?;
        if root.scheme() != target.scheme()
            || root.host_str() != target.host_str()
            || root.port() != target.port()
        {
            return None;
        }
        let root_parts: Vec<&str> = root.path_segments()?.filter(|s| !s.is_empty()).collect();
        let target_parts: Vec<&str> = target.path_segments()?.filter(|s| !s.is_empty()).collect();
        let rest = target_parts.strip_prefix(root_parts.as_slice())?;
        let decoded: Vec<String> = rest
            .iter()
            .map(|part| {
                percent_encoding::percent_decode_str(part)
                    .decode_utf8()
                    .map(|text| text.into_owned())
            })
            .collect::<Result<_, _>>()
            .ok()?;
        let path = decoded.join("/");
        // Re-resolve through the checked path, so a link or hidden folder is refused here too.
        let located = self.locate(&path).await.ok()?;
        (located.uri.trim_end_matches('/') == location.uri.trim_end_matches('/')).then_some(path)
    }

    async fn list_all(
        &self,
        folder: &Location,
        cancellation: &CancellationToken,
    ) -> Vec<EntrySummary> {
        let mut entries = Vec::new();
        let mut token = None;
        loop {
            let Ok(page) = self
                .provider
                .list(
                    folder,
                    ListOptions {
                        page_size: PAGE_SIZE,
                        continuation_token: token.take(),
                    },
                    cancellation.clone(),
                )
                .await
            else {
                break;
            };
            entries.extend(page.entries);
            if !page.has_more || page.continuation_token.is_none() || entries.len() > MAX_WALK_FILES
            {
                break;
            }
            token = page.continuation_token;
        }
        entries
    }

    async fn inspect(&self, location: &Location) -> Option<EntrySummary> {
        let entry = EntryRef {
            id: EntryId::new(),
            location: location.clone(),
        };
        self.provider
            .inspect(&entry, CancellationToken::new())
            .await
            .ok()
    }
}

/// The parts of a relative path; `None` for an empty, absolute or escaping path, or one through
/// a hidden or credential folder.
fn path_parts(path: &str) -> Option<Vec<String>> {
    let trimmed = path.trim();
    if trimmed.is_empty() || trimmed.starts_with('/') || trimmed.starts_with('\\') {
        return None;
    }
    if trimmed.len() > 1 && trimmed.as_bytes()[1] == b':' {
        return None;
    }
    let parts: Vec<String> = trimmed
        .split(['/', '\\'])
        .filter(|part| !part.is_empty() && *part != ".")
        .map(str::to_owned)
        .collect();
    let (_, folders) = parts.split_last()?;
    if parts.iter().any(|part| part == "..")
        || folders.iter().any(|folder| is_skipped_folder(folder))
    {
        return None;
    }
    Some(parts)
}

fn normalised(path: &str) -> String {
    path_parts(path).map_or_else(|| path.trim().to_owned(), |parts| parts.join("/"))
}

/// Folders a walk never enters: hidden ones (`.obsidian`, `.git`, `.trash`) and credential
/// folders.
fn is_skipped_folder(name: &str) -> bool {
    name.starts_with('.') || is_credential_folder(name) || name == "node_modules"
}

/// Whether a file name has a note extension.
pub(crate) fn is_note_name(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(stem, extension)| {
        !stem.is_empty() && NOTE_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str())
    })
}

#[cfg(test)]
pub(crate) mod tests;
