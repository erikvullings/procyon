//! `edit_file` (task 0352): changes an existing text file by replacing exact text, as Pi's `edit`
//! does. The tool only checks its arguments and interrupts the turn with an [`EditRequest`]; the
//! host reads the file, applies the edit with [`apply_edit`], shows the person the change and
//! writes it once they approve. So an edit never happens unseen, whatever a document the model
//! read told it to do.

use async_trait::async_trait;
use schemars::JsonSchema;
use serde::Deserialize;
use thiserror::Error;

use crate::paths::is_secret_name;
use crate::tool::{ToolContext, ToolEffect, ToolOutcome, TypedTool};

/// Longest replacement text, in bytes.
pub const MAX_EDIT_TEXT_BYTES: usize = 64 * 1024;
/// Largest file an edit may change, in bytes.
pub const MAX_EDITED_FILE_BYTES: usize = 1024 * 1024;

/// A change the model asks to make: the interrupt of `edit_file`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditRequest {
    /// File relative to the working folder.
    pub path: String,
    /// The exact text to replace.
    pub old_text: String,
    /// What replaces it.
    pub new_text: String,
    /// Replace every occurrence instead of exactly one.
    pub replace_all: bool,
}

/// Why an edit cannot be applied to a file's text. Shown to the model, which may try again.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum EditError {
    /// The text does not occur; `closest` is the most similar line, to help the next try.
    #[error("the text to replace was not found in the file.{}", closest_hint(.closest.as_ref()))]
    NotFound {
        /// The 1-based number and text of the line that looks most like the first line sought.
        closest: Option<(usize, String)>,
    },
    /// The text occurs more than once and `replace_all` was not set.
    #[error(
        "the text to replace occurs {count} times; include more of the surrounding text so it \
         occurs once, or set replace_all."
    )]
    Ambiguous {
        /// How often it occurs.
        count: usize,
    },
    /// The replacement would leave the file as it was.
    #[error("the new text is the same as the old text.")]
    Unchanged,
}

fn closest_hint(closest: Option<&(usize, String)>) -> String {
    closest.map_or_else(String::new, |(line, text)| {
        format!(" The closest line is {line}: {text}")
    })
}

/// The file's text with `edit` applied. A file with Windows line endings is matched and changed
/// with them, so the model may write `\n` as usual.
pub fn apply_edit(content: &str, edit: &EditRequest) -> Result<String, EditError> {
    let crlf = content.contains("\r\n");
    let lines = |text: &str| {
        if crlf && !text.contains("\r\n") {
            text.replace('\n', "\r\n")
        } else {
            text.to_owned()
        }
    };
    let old = lines(&edit.old_text);
    let new = lines(&edit.new_text);
    if old == new {
        return Err(EditError::Unchanged);
    }
    let count = content.matches(old.as_str()).count();
    match count {
        0 => Err(EditError::NotFound {
            closest: closest_line(content, &edit.old_text),
        }),
        1 => Ok(content.replacen(old.as_str(), &new, 1)),
        _ if edit.replace_all => Ok(content.replace(old.as_str(), &new)),
        _ => Err(EditError::Ambiguous { count }),
    }
}

/// The line of `content` sharing the most words with the first non-blank line of `sought`.
fn closest_line(content: &str, sought: &str) -> Option<(usize, String)> {
    let first = sought
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())?;
    let words: Vec<&str> = first.split_whitespace().collect();
    content
        .lines()
        .enumerate()
        .map(|(index, line)| {
            let shared = line
                .split_whitespace()
                .filter(|word| words.contains(word))
                .count();
            (shared, index, line)
        })
        .filter(|(shared, ..)| *shared > 0)
        .max_by_key(|(shared, index, _)| (*shared, std::cmp::Reverse(*index)))
        .map(|(_, index, line)| {
            (
                index + 1,
                crate::text::truncate_to_bytes(line.trim(), 200).to_owned(),
            )
        })
}

/// The arguments of `edit_file`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct EditFileArgs {
    /// File relative to the working folder.
    path: String,
    /// The exact text to replace, with enough surrounding lines to occur once in the file.
    old_text: String,
    /// The text that replaces it.
    new_text: String,
    /// Replace every occurrence instead of exactly one.
    replace_all: Option<bool>,
}

/// Changes an existing text file; the host shows the change and asks the person first.
pub struct EditFile;

#[async_trait]
impl<I: From<EditRequest> + Send + 'static> TypedTool<I> for EditFile {
    type Args = EditFileArgs;

    fn name(&self) -> &'static str {
        "edit_file"
    }

    fn description(&self) -> &'static str {
        "Changes an existing text file by replacing exact text. Read the file first and copy the \
         text to replace exactly, with enough surrounding lines that it occurs once. Several \
         changes to one file are several calls. The person sees each change and may decline it."
    }

    fn effect(&self) -> ToolEffect {
        ToolEffect::Exclusive
    }

    async fn run_typed(&self, _ctx: &ToolContext<'_>, args: EditFileArgs) -> ToolOutcome<I> {
        let path = args.path.trim();
        let step = format!("Could not edit {path}");
        if path.is_empty() {
            return ToolOutcome::error("Edit a file", "path must not be empty.");
        }
        let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
        if is_secret_name(name) {
            return ToolOutcome::error(step, format!("{path} may hold secrets and is not edited."));
        }
        if args.old_text.is_empty() {
            return ToolOutcome::error(
                step,
                "old_text must not be empty; to create a file use write_file.",
            );
        }
        if args.old_text.len() > MAX_EDIT_TEXT_BYTES || args.new_text.len() > MAX_EDIT_TEXT_BYTES {
            return ToolOutcome::error(
                step,
                format!(
                    "old_text and new_text may be at most {} KiB each; split the change.",
                    MAX_EDIT_TEXT_BYTES / 1024
                ),
            );
        }
        ToolOutcome::Interrupt(I::from(EditRequest {
            path: path.to_owned(),
            old_text: args.old_text,
            new_text: args.new_text,
            replace_all: args.replace_all.unwrap_or(false),
        }))
    }
}

#[cfg(test)]
mod tests;
