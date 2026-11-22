//! The assistant's one write tool (task 0338): create a new `.md` or `.txt` file in a fixed output
//! folder under the working folder. It can never replace a file, never leaves that folder and
//! never writes names that start programs or hold credentials, so it stays harmless even when a
//! document the assistant read tells it to write somewhere else. It works through the VFS
//! provider, so a local and an SSH folder behave the same.

use std::sync::Arc;

use fm_domain::{EntryId, EntryKind, Location};
use fm_vfs::{EntryRef, FileSystemProvider, ProviderRegistry, VfsError, WriteOptions};
use thiserror::Error;
use tokio::io::AsyncWriteExt as _;
use tokio_util::sync::CancellationToken;

use fm_agent::is_secret_name;
use fm_agent::redact_secrets;

/// The folder under the working folder where files are created.
pub(super) const OUTPUT_FOLDER: &str = "summaries";
/// Largest file the assistant creates.
pub(super) const MAX_FILE_BYTES: usize = 64 * 1024;
/// Most files created for one question.
pub(super) const MAX_FILES_PER_QUESTION: usize = 50;
/// Most bytes created for one question, all files together.
pub(super) const MAX_BYTES_PER_QUESTION: usize = 1024 * 1024;
/// Longest file name, in characters.
const MAX_NAME_CHARS: usize = 100;
/// Name suffixes tried before giving up when the name is taken.
const MAX_SUFFIX: u32 = 99;
/// Extensions the assistant may create.
const ALLOWED_EXTENSIONS: &[&str] = &["md", "txt"];

/// Why a file was not created. Shown to the model as `Error: ...`.
#[derive(Debug, Error, PartialEq, Eq)]
pub(super) enum WriteError {
    #[error(
        "{0} is not an allowed file name. Use a short plain name such as summary.md: no folders, \
         no leading dot, only .md or .txt."
    )]
    Name(String),
    #[error("The file is larger than {} KiB.", MAX_FILE_BYTES / 1024)]
    TooLarge,
    #[error(
        "The limit for this question is reached ({MAX_FILES_PER_QUESTION} files or {} KiB). \
         Ask the person before creating more.",
        MAX_BYTES_PER_QUESTION / 1024
    )]
    QuestionLimit,
    #[error("The output folder {OUTPUT_FOLDER}/ is not usable.")]
    Folder,
    #[error("{0} could not be created.")]
    Failed(String),
}

/// A file that was created.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Created {
    /// The name it got, which differs from the requested one when that was taken.
    pub(super) name: String,
    pub(super) bytes: usize,
    /// Values replaced because they looked like secrets.
    pub(super) redacted: usize,
}

impl Created {
    /// The line for the person.
    pub(super) fn step(&self) -> String {
        format!(
            "Created {OUTPUT_FOLDER}/{} ({})",
            self.name,
            size_text(self.bytes)
        )
    }

    /// The text for the model: the final name, which may differ from the requested one.
    pub(super) fn result(&self) -> String {
        let mut text = format!(
            "Created {OUTPUT_FOLDER}/{} ({}). Existing files are never changed.",
            self.name,
            size_text(self.bytes)
        );
        if self.redacted > 0 {
            text.push_str(&format!(
                " {} value(s) that looked like secrets were replaced with [redacted].",
                self.redacted
            ));
        }
        text
    }
}

fn size_text(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    }
}

/// Whether `name` is a plain file name the assistant may create.
pub(super) fn valid_name(name: &str) -> bool {
    let trimmed = name.trim();
    let Some((stem, extension)) = trimmed.rsplit_once('.') else {
        return false;
    };
    !stem.is_empty()
        && trimmed == name
        && trimmed.chars().count() <= MAX_NAME_CHARS
        && !trimmed.starts_with('.')
        && !trimmed.contains("..")
        && !trimmed
            .chars()
            .any(|c| c.is_control() || "/\\:*?\"<>|".contains(c))
        && ALLOWED_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str())
        && !is_secret_name(trimmed)
        && !stem.to_ascii_lowercase().contains("secret")
        && !stem.to_ascii_lowercase().contains("password")
        && !stem.to_ascii_lowercase().contains("credential")
}

/// Whether a file with this name and content may be created at all.
pub(super) fn check(name: &str, content: &str) -> Result<(), WriteError> {
    if !valid_name(name) {
        return Err(WriteError::Name(name.to_owned()));
    }
    if content.len() > MAX_FILE_BYTES {
        return Err(WriteError::TooLarge);
    }
    Ok(())
}

/// Creates files in the output folder of one working folder.
#[derive(Clone)]
pub(super) struct FileWriter {
    provider: Arc<dyn FileSystemProvider>,
    root: Location,
}

impl FileWriter {
    /// `None` when no provider serves the working folder.
    pub(super) fn open(providers: &ProviderRegistry, working_directory: &str) -> Option<Self> {
        let root = providers.parse(working_directory).ok()?;
        let provider = providers.resolve(&root).ok()?;
        Some(Self { provider, root })
    }

    /// Creates `name` with `content` in the output folder. The folder is created when missing;
    /// a name that is taken gets `-2`, `-3`, ... before its extension.
    pub(super) async fn create(&self, name: &str, content: &str) -> Result<Created, WriteError> {
        check(name, content)?;
        let folder = self.output_folder().await?;
        let text = redact_secrets(content);
        let redacted = text
            .matches("[redacted")
            .count()
            .saturating_sub(content.matches("[redacted").count());
        let (stem, extension) = name.rsplit_once('.').unwrap_or((name, "md"));
        for attempt in 1..=MAX_SUFFIX {
            let candidate = if attempt == 1 {
                name.to_owned()
            } else {
                format!("{stem}-{attempt}.{extension}")
            };
            let destination = folder
                .join(&candidate)
                .map_err(|_| WriteError::Name(name.to_owned()))?;
            match self
                .provider
                .open_write(
                    &destination,
                    WriteOptions { overwrite: false },
                    CancellationToken::new(),
                )
                .await
            {
                Ok(mut stream) => {
                    let written = async {
                        stream.write_all(text.as_bytes()).await?;
                        stream.shutdown().await
                    }
                    .await;
                    return written
                        .map(|()| Created {
                            name: candidate.clone(),
                            bytes: text.len(),
                            redacted,
                        })
                        .map_err(|_| WriteError::Failed(candidate));
                }
                Err(VfsError::AlreadyExists { .. }) => continue,
                Err(_) => return Err(WriteError::Failed(candidate)),
            }
        }
        Err(WriteError::Failed(name.to_owned()))
    }

    /// The output folder, created when it is missing. A link or a file of that name is refused.
    async fn output_folder(&self) -> Result<Location, WriteError> {
        let folder = self
            .root
            .join(OUTPUT_FOLDER)
            .map_err(|_| WriteError::Folder)?;
        let entry = EntryRef {
            id: EntryId::new(),
            location: folder.clone(),
        };
        match self
            .provider
            .inspect(&entry, CancellationToken::new())
            .await
        {
            Ok(summary) if summary.kind == EntryKind::Directory => Ok(folder),
            Ok(_) => Err(WriteError::Folder),
            Err(VfsError::NotFound { .. }) => {
                self.provider
                    .create_directory(&self.root, OUTPUT_FOLDER, CancellationToken::new())
                    .await
                    .map_err(|_| WriteError::Folder)?;
                Ok(folder)
            }
            Err(_) => Err(WriteError::Folder),
        }
    }
}

#[cfg(test)]
mod tests;
