//! Bounded, create-only Markdown and text writes in the terminal working directory.

use std::sync::Arc;

use fm_domain::Location;
use fm_vfs::{FileSystemProvider, ProviderRegistry, VfsError, WriteOptions};
use thiserror::Error;
use tokio::io::AsyncWriteExt as _;
use tokio_util::sync::CancellationToken;

use fm_agent::is_secret_name;
use fm_agent::redact_secrets;

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
    #[error("The working folder is not usable.")]
    Folder,
    #[error("{0} could not be created.")]
    Failed(String),
    #[error("{0} already exists. Use edit_file to propose a change to it.")]
    Exists(String),
}

/// A file that was created.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Created {
    /// The name it got, which differs from the requested one when that was taken.
    pub(super) name: String,
    pub(super) tool_path: String,
    pub(super) bytes: usize,
    /// Values replaced because they looked like secrets.
    pub(super) redacted: usize,
}

impl Created {
    /// The line for the person.
    pub(super) fn step(&self) -> String {
        format!("Created ./{} ({})", self.name, size_text(self.bytes))
    }

    /// The text for the model: the final name, which may differ from the requested one.
    pub(super) fn result(&self) -> String {
        let mut text = format!(
            "Created {} ({}). Use this path with read_file or edit_file.",
            self.tool_path,
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

/// Recognises a direct creation instruction naming this destination in the person's question.
pub(super) fn explicit_destination(question: &str, name: &str) -> bool {
    question
        .split_inclusive(['\n', ';', '!', '?'])
        .any(|clause| {
            let words: Vec<_> = clause.split_whitespace().collect();
            !words.iter().any(|word| {
                matches!(
                    word.to_ascii_lowercase().as_str(),
                    "not" | "don't" | "never" | "without" | "if"
                )
            }) && words.iter().enumerate().any(|(index, word)| {
                if !matches!(
                    word.to_ascii_lowercase().as_str(),
                    "write" | "create" | "save"
                ) {
                    return false;
                }
                let tail = &words[index + 1..];
                let is_name = |word: &str| {
                    let word = word
                        .trim_end_matches(['.', ',', '!', '?', ';'])
                        .trim_matches(['`', '"', '\'']);
                    word == name || word.strip_prefix("./") == Some(name)
                };
                tail.first().is_some_and(|word| is_name(word))
                    || tail.windows(2).any(|pair| {
                        matches!(pair[0].to_ascii_lowercase().as_str(), "to" | "into" | "as")
                            && is_name(pair[1])
                    })
            })
        })
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

/// Creates files in the shell working directory.
#[derive(Clone)]
pub(super) struct FileWriter {
    provider: Arc<dyn FileSystemProvider>,
    root: Location,
    tool_prefix: String,
}

impl FileWriter {
    /// `None` when no provider serves the working folder.
    pub(super) fn open(
        providers: &ProviderRegistry,
        working_directory: &str,
        tools_folder: &str,
    ) -> Option<Self> {
        let root = providers.parse(working_directory).ok()?;
        let provider = providers.resolve(&root).ok()?;
        let relative = working_directory
            .trim_end_matches('/')
            .strip_prefix(tools_folder.trim_end_matches('/'))
            .unwrap_or_default()
            .trim_matches('/');
        let tool_prefix = percent_encoding::percent_decode_str(relative)
            .decode_utf8_lossy()
            .into_owned();
        Some(Self {
            provider,
            root,
            tool_prefix,
        })
    }

    /// Creates a new file, optionally choosing a numbered suffix on collision.
    pub(super) async fn create(
        &self,
        name: &str,
        content: &str,
        allow_suffix: bool,
    ) -> Result<Created, WriteError> {
        check(name, content)?;
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
            let destination = self
                .root
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
                            tool_path: if self.tool_prefix.is_empty() {
                                candidate.clone()
                            } else {
                                format!("{}/{candidate}", self.tool_prefix)
                            },
                            name: candidate.clone(),
                            bytes: text.len(),
                            redacted,
                        })
                        .map_err(|_| WriteError::Failed(candidate));
                }
                Err(VfsError::AlreadyExists { .. }) if allow_suffix => continue,
                Err(VfsError::AlreadyExists { .. }) => return Err(WriteError::Exists(candidate)),
                Err(_) => return Err(WriteError::Failed(candidate)),
            }
        }
        Err(WriteError::Failed(name.to_owned()))
    }
}

#[cfg(test)]
mod tests;
