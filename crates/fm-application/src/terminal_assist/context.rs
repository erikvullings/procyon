//! What the assistant is told about the person's terminal (task 0336): the system, the folder, the
//! selection and the recent commands, rendered as text and with secrets removed.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use fm_transport_dto::{TerminalAssistRequestDto, TerminalBlockDto};

use super::redact::redact_secrets;
use super::{newest_bytes, truncate_to_bytes};

/// Most folder names listed.
const MAX_LISTED_NAMES: usize = 1000;
/// Most selected names listed.
const MAX_SELECTED_NAMES: usize = 50;
/// Most output bytes kept for one command.
const MAX_BLOCK_OUTPUT_BYTES: usize = 4 * 1024;
/// Most bytes for all recent commands together; the oldest go first.
const MAX_BLOCKS_BYTES: usize = 10 * 1024;
/// Most bytes kept for one attached file.
const MAX_FILE_BYTES: usize = 24 * 1024;

/// A file the person named with `@path`, already read inside the working folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AttachedFile {
    pub(super) name: String,
    pub(super) text: String,
}

/// The local path a `file://` location URI names. `None` for any other kind of location.
pub(super) fn path_from_location(uri: &str) -> Option<PathBuf> {
    url::Url::parse(uri).ok()?.to_file_path().ok()
}

/// The request's whole context as prompt text: the state, then the activity. Used by the one-shot
/// kinds; a session sends the two separately.
pub(super) fn render_context(request: &TerminalAssistRequestDto, files: &[AttachedFile]) -> String {
    let state = render_state(request);
    let activity = render_activity(&request.context.blocks, files);
    if activity.is_empty() {
        state
    } else {
        format!("{state}\n{activity}")
    }
}

/// What describes the terminal now, as one text.
pub(super) fn render_state(request: &TerminalAssistRequestDto) -> String {
    state_sections(request)
        .into_iter()
        .map(|(_, text)| text)
        .collect::<Vec<_>>()
        .join("\n")
}

/// What describes the terminal now, in parts that change independently: `header` (system, shell,
/// folder, branch), `otherPane`, `selection` and `listing`. A session sends a part with the first
/// question and again only when it changes.
pub(super) fn state_sections(request: &TerminalAssistRequestDto) -> Vec<(&'static str, String)> {
    let context = &request.context;
    let folder = path_from_location(&request.working_directory);
    let shown_path = folder.clone();
    let mut header = String::new();
    if folder.is_some() && !context.omit_environment {
        let _ = writeln!(
            header,
            "Operating system: {} ({})",
            std::env::consts::OS,
            std::env::consts::ARCH
        );
    }
    let _ = writeln!(
        header,
        "Shell: {}",
        line(request.shell.as_deref().unwrap_or("unknown"))
    );
    let shown_folder = folder.map_or_else(
        || line(&request.working_directory),
        |path| line(&path.to_string_lossy()),
    );
    let _ = writeln!(header, "Working directory: {shown_folder}");
    if let Some(branch) = shown_path.as_deref().and_then(git_branch) {
        let _ = writeln!(header, "Git branch: {}", line(&branch));
    }
    let mut sections = vec![("header", header.trim_end().to_owned())];
    if let Some(other) = context.other_folder.as_deref() {
        let other = path_from_location(other)
            .map_or_else(|| line(other), |path| line(&path.to_string_lossy()));
        sections.push(("otherPane", format!("Other pane folder: {other}")));
    }
    if !context.selected.is_empty() {
        let names: Vec<String> = context
            .selected
            .iter()
            .take(MAX_SELECTED_NAMES)
            .map(|name| line(name))
            .collect();
        sections.push((
            "selection",
            format!("Selected in the folder: {}", names.join(", ")),
        ));
    }
    if !context.listing.is_empty() {
        let mut listing = format!("Folder listing ({} names):", context.listing.len());
        for name in context.listing.iter().take(MAX_LISTED_NAMES) {
            let _ = write!(listing, "\n- {}", line(name));
        }
        if context.listing.len() > MAX_LISTED_NAMES {
            let _ = write!(
                listing,
                "\n- and {} more",
                context.listing.len() - MAX_LISTED_NAMES
            );
        }
        sections.push(("listing", listing));
    }
    sections
}

/// What happened since: recent commands with their output, and the files named with `@path`.
pub(super) fn render_activity(blocks: &[TerminalBlockDto], files: &[AttachedFile]) -> String {
    let mut text = String::new();
    let blocks = render_blocks(blocks);
    if !blocks.is_empty() {
        let _ = write!(text, "Recent commands, oldest first:\n{blocks}");
    }
    for file in files {
        // The start of a named file matters most; the model can read the rest with `offset`.
        let kept = truncate_to_bytes(&file.text, MAX_FILE_BYTES);
        let body = redact_secrets(kept);
        let _ = write!(text, "File {}:\n{body}", line(&file.name));
        if kept.len() < file.text.len() {
            let _ = write!(
                text,
                "\n[cut at byte {}: read_file with \"offset\":{} continues]",
                kept.len(),
                kept.len()
            );
        }
        text.push('\n');
    }
    text.trim_end().to_owned()
}

/// The branch a folder's git checkout is on, read from `.git/HEAD` in the folder or above it.
/// `None` outside a checkout or on a detached head.
fn git_branch(folder: &Path) -> Option<String> {
    folder.ancestors().find_map(|dir| {
        let head = std::fs::read_to_string(dir.join(".git").join("HEAD")).ok()?;
        head.trim()
            .strip_prefix("ref: refs/heads/")
            .map(str::to_owned)
    })
}

/// Recent commands as text, newest kept when they do not all fit.
fn render_blocks(blocks: &[TerminalBlockDto]) -> String {
    let rendered: Vec<String> = blocks
        .iter()
        .map(|block| {
            let output = redact_secrets(newest_bytes(
                block.output.trim_end(),
                MAX_BLOCK_OUTPUT_BYTES,
            ));
            format!(
                "$ {} (exit {})\n{output}\n",
                redact_secrets(&line(&block.command)),
                block.exit_code
            )
        })
        .collect();
    let mut start = rendered.len();
    let mut total = 0;
    while start > 0 {
        let size = rendered[start - 1].len();
        if total + size > MAX_BLOCKS_BYTES && start < rendered.len() {
            break;
        }
        total += size;
        start -= 1;
    }
    rendered[start..].concat()
}

/// One line without control characters, for values that are not the person's own message.
fn line(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_control())
        .take(512)
        .collect()
}

#[cfg(test)]
mod tests;
