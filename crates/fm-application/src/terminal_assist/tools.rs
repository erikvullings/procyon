//! The agent's read-only tools (task 0336). They look only inside the terminal's working folder,
//! follow no symlink out of it, refuse files that usually hold secrets, and bound everything.

use std::fmt::Write as _;
use std::fs;
use std::io::{Read as _, Seek as _, SeekFrom};
use std::path::{Path, PathBuf};

use fm_vfs::ProviderRegistry;
use thiserror::Error;

use super::context::path_from_location;
use super::remote::RemoteSandbox;
use fm_agent::redact_secrets;
pub(super) use fm_agent::{is_credential_folder, is_secret_name};

/// Most bytes of one file returned.
pub(super) const MAX_READ_BYTES: usize = 32 * 1024;
/// Most folder entries listed.
pub(super) const MAX_LISTED: usize = 200;
/// Most matching lines returned.
const MAX_MATCHES: usize = 50;
/// Most files a search looks into.
const MAX_SEARCHED_FILES: usize = 2000;
/// Largest file a search reads.
const MAX_SEARCHED_FILE_BYTES: u64 = 1024 * 1024;
/// Deepest folder level a search enters, so a symlink loop cannot recurse without end.
const MAX_SEARCH_DEPTH: usize = 12;
/// Folders a search never enters.
const SKIPPED_FOLDERS: &[&str] = &[".git", "node_modules", "target", ".venv", "dist"];

/// One read-only file tool call, after the registry has checked the arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum FileCall {
    ListDir {
        path: String,
    },
    ReadFile {
        path: String,
        /// Byte offset to continue a file that an earlier read cut off.
        offset: Option<usize>,
    },
    Grep {
        pattern: String,
        path: Option<String>,
    },
}

/// Why a tool could not do what the model asked. Shown to the model as `Error: ...`; a missing
/// path and one outside the folder read the same, so the model learns nothing about other paths.
#[derive(Debug, Error)]
pub(super) enum ToolError {
    #[error("{0} is not available.")]
    NotAvailable(String),
    #[error("{0} is not a folder.")]
    NotFolder(String),
    #[error("{0} may hold secrets and is not read.")]
    Secret(String),
    #[error("{0} is not a regular text file.")]
    NotText(String),
    #[error("{0} cannot be read.")]
    Unreadable(String),
    #[error("{0}")]
    Document(String),
    #[error(
        "grep is not available for remote folders. Use read_file, or ask to run grep with run_command."
    )]
    RemoteGrep,
}

pub(super) fn error_text(error: &ToolError) -> String {
    format!("Error: {error}")
}

/// What a tool run produced: a line for the person and the text for the model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ToolOutput {
    pub(super) step: String,
    pub(super) text: String,
}

/// A working folder the tools may look into.
#[derive(Debug, Clone)]
pub(super) struct Sandbox {
    root: PathBuf,
}

impl Sandbox {
    /// `None` when the folder does not exist.
    pub(super) fn new(root: &Path) -> Option<Self> {
        Some(Self {
            root: root.canonicalize().ok()?,
        })
    }

    /// Runs a read-only tool: a one-line description for the person, and the text for the model.
    pub(super) fn run(&self, call: &FileCall) -> ToolOutput {
        let (step, result) = match call {
            FileCall::ListDir { path } => (format!("Listed {path}"), self.list_dir(path)),
            FileCall::ReadFile { path, offset } => (
                format!("Read {path}"),
                self.read_file(path, offset.unwrap_or(0)),
            ),
            FileCall::Grep { pattern, path } => (
                format!("Searched for {pattern}"),
                self.grep(pattern, path.as_deref().unwrap_or(".")),
            ),
        };
        ToolOutput {
            step,
            text: result.unwrap_or_else(|error| error_text(&error)),
        }
    }

    /// The text of a file inside the folder, for `@path` mentions. `None` when it cannot be read.
    pub(super) fn read_text(&self, relative: &str) -> Option<String> {
        let path = self.resolve(relative).ok()?;
        read_text_file(&path, 0).ok()
    }

    /// The `file://` URL of a regular, non-secret file inside the folder, for document reads.
    fn document_url(&self, relative: &str) -> Result<String, ToolError> {
        let path = self.resolve(relative)?;
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        if is_secret_name(&name) {
            return Err(ToolError::Secret(name));
        }
        if !fs::metadata(&path).is_ok_and(|metadata| metadata.is_file()) {
            return Err(ToolError::NotText(name));
        }
        url::Url::from_file_path(&path)
            .map(String::from)
            .map_err(|()| ToolError::Unreadable(name))
    }

    /// The canonical path of `relative`, which must stay inside the folder and avoid credential
    /// folders. A missing path and an outside path give the same error.
    fn resolve(&self, relative: &str) -> Result<PathBuf, ToolError> {
        let unavailable = || ToolError::NotAvailable(relative.to_owned());
        let candidate = Path::new(relative.trim());
        let joined = if candidate.is_absolute() {
            candidate.to_path_buf()
        } else {
            self.root.join(candidate)
        };
        let real = joined.canonicalize().map_err(|_| unavailable())?;
        let inside = real.strip_prefix(&self.root).map_err(|_| unavailable())?;
        if inside
            .components()
            .any(|part| is_credential_folder(&part.as_os_str().to_string_lossy()))
        {
            return Err(unavailable());
        }
        Ok(real)
    }

    fn list_dir(&self, relative: &str) -> Result<String, ToolError> {
        let path = self.resolve(relative)?;
        let entries = fs::read_dir(&path).map_err(|_| ToolError::NotFolder(relative.to_owned()))?;
        let mut names: Vec<String> = entries
            .flatten()
            .map(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                if entry.path().is_dir() {
                    format!("{name}/")
                } else {
                    name
                }
            })
            .collect();
        names.sort();
        let more = names.len().saturating_sub(MAX_LISTED);
        names.truncate(MAX_LISTED);
        if more > 0 {
            names.push(format!("... and {more} more"));
        }
        Ok(names.join("\n"))
    }

    fn read_file(&self, relative: &str, offset: usize) -> Result<String, ToolError> {
        let path = self.resolve(relative)?;
        Ok(redact_secrets(&read_text_file(&path, offset)?))
    }

    fn grep(&self, pattern: &str, relative: &str) -> Result<String, ToolError> {
        let start = self.resolve(relative)?;
        let needle = pattern.to_lowercase();
        let mut matches = Vec::new();
        let mut budget = MAX_SEARCHED_FILES;
        self.search(&start, &needle, &mut matches, &mut budget, 0);
        Ok(if matches.is_empty() {
            "No matches.".to_owned()
        } else {
            redact_secrets(&matches.join("\n"))
        })
    }

    fn search(
        &self,
        folder: &Path,
        needle: &str,
        matches: &mut Vec<String>,
        budget: &mut usize,
        depth: usize,
    ) {
        if depth > MAX_SEARCH_DEPTH {
            return;
        }
        let mut entries: Vec<PathBuf> = if folder.is_dir() {
            match fs::read_dir(folder) {
                Ok(entries) => entries.flatten().map(|entry| entry.path()).collect(),
                Err(_) => return,
            }
        } else {
            vec![folder.to_path_buf()]
        };
        entries.sort();
        for path in entries {
            if matches.len() >= MAX_MATCHES || *budget == 0 {
                return;
            }
            // A symlink is followed only when it stays inside the folder; names are the real ones.
            let Ok(real) = path.canonicalize() else {
                continue;
            };
            let Ok(inside) = real.strip_prefix(&self.root) else {
                continue;
            };
            if inside
                .components()
                .any(|part| is_credential_folder(&part.as_os_str().to_string_lossy()))
            {
                continue;
            }
            let name = real
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            if real.is_dir() {
                if !SKIPPED_FOLDERS.contains(&name.as_str()) {
                    self.search(&real, needle, matches, budget, depth + 1);
                }
                continue;
            }
            if is_secret_name(&name)
                || fs::metadata(&real).map_or(true, |meta| meta.len() > MAX_SEARCHED_FILE_BYTES)
            {
                continue;
            }
            *budget -= 1;
            let Ok(text) = read_text_file(&real, 0) else {
                continue;
            };
            let shown = real
                .strip_prefix(&self.root)
                .unwrap_or(&real)
                .display()
                .to_string();
            for (number, text_line) in text.lines().enumerate() {
                if text_line.to_lowercase().contains(needle) {
                    matches.push(format!("{shown}:{}: {}", number + 1, text_line.trim()));
                    if matches.len() >= MAX_MATCHES {
                        return;
                    }
                }
            }
        }
    }
}

/// A bounded regular text file. Special files (a FIFO would block), binary files and
/// secret-looking names are refused.
fn read_text_file(path: &Path, offset: usize) -> Result<String, ToolError> {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    if is_secret_name(&name) {
        return Err(ToolError::Secret(name));
    }
    let metadata = fs::metadata(path).map_err(|_| ToolError::Unreadable(name.clone()))?;
    if !metadata.is_file() {
        return Err(ToolError::NotText(name));
    }
    let mut file = fs::File::open(path).map_err(|_| ToolError::Unreadable(name.clone()))?;
    if offset > 0 {
        file.seek(SeekFrom::Start(offset as u64))
            .map_err(|_| ToolError::Unreadable(name.clone()))?;
    }
    let mut buffer = Vec::new();
    file.take(MAX_READ_BYTES as u64 + 1)
        .read_to_end(&mut buffer)
        .map_err(|_| ToolError::Unreadable(name.clone()))?;
    text_from_bytes(buffer, &name, offset)
}

/// The text of at most `MAX_READ_BYTES + 1` bytes read from a file: binary content is refused and
/// anything past the limit is cut with a marker.
pub(super) fn text_from_bytes(
    mut buffer: Vec<u8>,
    name: &str,
    offset: usize,
) -> Result<String, ToolError> {
    if buffer.contains(&0) {
        return Err(ToolError::NotText(name.to_owned()));
    }
    let truncated = buffer.len() > MAX_READ_BYTES;
    buffer.truncate(MAX_READ_BYTES);
    let mut text = String::from_utf8_lossy(&buffer).into_owned();
    if truncated {
        let _ = write!(
            text,
            "\n[truncated at byte {next}: call read_file with \"offset\":{next} to continue]",
            next = offset + MAX_READ_BYTES
        );
    }
    Ok(text)
}

/// Where the tools look: a local folder, or one on another machine through its provider.
#[derive(Clone)]
pub(super) enum ToolHost {
    Local(Sandbox),
    Remote(RemoteSandbox),
}

impl ToolHost {
    /// The location of a document file the tools may read, checked like `read_file`.
    pub(super) async fn document_location(
        &self,
        providers: &ProviderRegistry,
        relative: &str,
    ) -> Result<fm_domain::Location, ToolError> {
        match self {
            Self::Local(sandbox) => {
                let url = sandbox.document_url(relative)?;
                providers
                    .parse(&url)
                    .map_err(|_| ToolError::Unreadable(relative.to_owned()))
            }
            Self::Remote(remote) => remote.document_location(relative).await,
        }
    }

    /// `None` when the working folder cannot be reached.
    pub(super) fn open(providers: &ProviderRegistry, working_directory: &str) -> Option<Self> {
        match path_from_location(working_directory) {
            Some(path) => Sandbox::new(&path).map(Self::Local),
            None => RemoteSandbox::new(providers, working_directory).map(Self::Remote),
        }
    }

    /// Runs one read-only tool.
    pub(super) async fn run(&self, call: FileCall) -> ToolOutput {
        match self {
            Self::Local(sandbox) => {
                let sandbox = sandbox.clone();
                tokio::task::spawn_blocking(move || sandbox.run(&call))
                    .await
                    .unwrap_or_else(|_| ToolOutput {
                        step: "Tool".to_owned(),
                        text: "Error: the tool failed.".to_owned(),
                    })
            }
            Self::Remote(remote) => remote.run(&call).await,
        }
    }

    /// The text of a file inside the working folder, for `@path` mentions.
    pub(super) async fn read_text(&self, relative: &str) -> Option<String> {
        match self {
            Self::Local(sandbox) => {
                let sandbox = sandbox.clone();
                let relative = relative.to_owned();
                tokio::task::spawn_blocking(move || sandbox.read_text(&relative))
                    .await
                    .ok()
                    .flatten()
            }
            Self::Remote(remote) => remote.read_text(relative).await,
        }
    }
}

#[cfg(test)]
mod tests;
