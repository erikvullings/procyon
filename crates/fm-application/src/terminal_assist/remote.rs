//! The read-only tools for a folder on another machine, reached through its VFS provider
//! (task 0336), so `list_dir` and `read_file` work in an SSH terminal too. The same rules apply as
//! for a local folder: nothing outside the working folder, no symlinks followed, no credential
//! folders or secret-looking files, bounded output. `grep` would need a remote tree walk, so it is
//! not offered here.

use std::sync::Arc;

use fm_domain::{EntryId, EntryKind, EntrySummary, Location};
use fm_vfs::{EntryRef, FileSystemProvider, ListOptions, ProviderRegistry};
use tokio::io::AsyncReadExt as _;
use tokio_util::sync::CancellationToken;

use super::protocol::ToolCall;
use super::redact::redact_secrets;
use super::tools::{
    MAX_LISTED, MAX_READ_BYTES, ToolError, ToolOutput, error_text, is_credential_folder,
    is_secret_name, text_from_bytes,
};

/// A working folder on a provider other than the local disk.
#[derive(Clone)]
pub(super) struct RemoteSandbox {
    provider: Arc<dyn FileSystemProvider>,
    root: Location,
}

impl RemoteSandbox {
    /// `None` when no provider serves the location.
    pub(super) fn new(providers: &ProviderRegistry, working_directory: &str) -> Option<Self> {
        let root = providers.parse(working_directory).ok()?;
        let provider = providers.resolve(&root).ok()?;
        Some(Self { provider, root })
    }

    /// Runs a read-only tool: a one-line description for the person, and the text for the model.
    pub(super) async fn run(&self, call: &ToolCall) -> ToolOutput {
        let (step, result) = match call {
            ToolCall::ListDir { path } => (format!("Listed {path}"), self.list_dir(path).await),
            ToolCall::ReadFile { path } => (format!("Read {path}"), self.read_file(path).await),
            ToolCall::Grep { pattern, .. } => (
                format!("Searched for {pattern}"),
                Err(ToolError::RemoteGrep),
            ),
            ToolCall::RunCommand { .. } => {
                ("Run command".to_owned(), Err(ToolError::NeedsApproval))
            }
            ToolCall::SearchNotes { .. }
            | ToolCall::ReadNote { .. }
            | ToolCall::ProposeNote { .. }
            | ToolCall::ProposeNoteUpdate { .. } => ("Notes".to_owned(), Err(ToolError::NotesTool)),
        };
        ToolOutput {
            step,
            text: result.unwrap_or_else(|error| error_text(&error)),
        }
    }

    /// The text of a file inside the folder, for `@path` mentions.
    pub(super) async fn read_text(&self, relative: &str) -> Option<String> {
        self.read_file_text(relative).await.ok()
    }

    /// The location of `relative` inside the folder and what it is. The path is checked piece by
    /// piece without following links: `..`, absolute paths, credential folders and symlinks are
    /// all "not available", and so is a path that does not exist.
    async fn locate(&self, relative: &str) -> Result<(Location, EntrySummary), ToolError> {
        let unavailable = || ToolError::NotAvailable(relative.to_owned());
        let relative = relative.trim();
        if relative.starts_with('/') {
            return Err(unavailable());
        }
        let mut location = self.root.clone();
        let mut summary = self.inspect(&location).await.map_err(|()| unavailable())?;
        for name in relative
            .split('/')
            .filter(|part| !part.is_empty() && *part != ".")
        {
            if name == ".." || is_credential_folder(name) {
                return Err(unavailable());
            }
            location = location.join(name).map_err(|_| unavailable())?;
            summary = self.inspect(&location).await.map_err(|()| unavailable())?;
            if summary.kind == EntryKind::Symlink {
                return Err(unavailable());
            }
        }
        Ok((location, summary))
    }

    async fn inspect(&self, location: &Location) -> Result<EntrySummary, ()> {
        let entry = EntryRef {
            id: EntryId::new(),
            location: location.clone(),
        };
        self.provider
            .inspect(&entry, CancellationToken::new())
            .await
            .map_err(|_| ())
    }

    async fn list_dir(&self, relative: &str) -> Result<String, ToolError> {
        let (location, summary) = self.locate(relative).await?;
        if summary.kind != EntryKind::Directory {
            return Err(ToolError::NotFolder(relative.to_owned()));
        }
        let page = self
            .provider
            .list(
                &location,
                ListOptions {
                    page_size: MAX_LISTED,
                    continuation_token: None,
                },
                CancellationToken::new(),
            )
            .await
            .map_err(|_| ToolError::NotFolder(relative.to_owned()))?;
        let mut names: Vec<String> = page
            .entries
            .iter()
            .map(|entry| {
                if entry.kind == EntryKind::Directory {
                    format!("{}/", entry.name)
                } else {
                    entry.name.clone()
                }
            })
            .collect();
        names.sort();
        if page.has_more {
            names.push("... and more".to_owned());
        }
        Ok(names.join("\n"))
    }

    async fn read_file(&self, relative: &str) -> Result<String, ToolError> {
        Ok(redact_secrets(&self.read_file_text(relative).await?))
    }

    async fn read_file_text(&self, relative: &str) -> Result<String, ToolError> {
        let (location, summary) = self.locate(relative).await?;
        let name = summary.name.clone();
        if is_secret_name(&name) {
            return Err(ToolError::Secret(name));
        }
        if summary.kind != EntryKind::File {
            return Err(ToolError::NotText(name));
        }
        let entry = EntryRef {
            id: EntryId::new(),
            location,
        };
        let stream = self
            .provider
            .open_read(&entry, CancellationToken::new())
            .await
            .map_err(|_| ToolError::Unreadable(name.clone()))?;
        let mut buffer = Vec::new();
        stream
            .take(MAX_READ_BYTES as u64 + 1)
            .read_to_end(&mut buffer)
            .await
            .map_err(|_| ToolError::Unreadable(name.clone()))?;
        text_from_bytes(buffer, &name)
    }
}

#[cfg(test)]
mod tests;
