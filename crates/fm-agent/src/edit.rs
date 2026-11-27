//! `edit_file` (task 0352): changes an existing text file by replacing text. The tool only checks its arguments and interrupts the turn with an [`EditRequest`]; the
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
    /// The exact text to replace. Empty for an explicit prepend operation.
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
    #[error("the text to replace was not found in the file.{} Trailing-whitespace matches: {trailing}. Indentation matches: {indentation}. Read the file again and copy a unique block.", closest_hint(.closest.as_ref()))]
    NotFound {
        /// The 1-based number and text of the line that looks most like the first line sought.
        closest: Option<(usize, String)>,
        /// Candidates ignoring trailing whitespace.
        trailing: usize,
        /// Candidates after a uniform indentation shift.
        indentation: usize,
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

/// The tier that matched the requested text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditMatch {
    /// Verbatim replacement.
    Exact,
    /// Differences in trailing whitespace only.
    TrailingWhitespace,
    /// Differences in indentation as well as trailing whitespace.
    Indentation,
}

impl std::fmt::Display for EditMatch {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Exact => "exact",
            Self::TrailingWhitespace => "ignoring trailing whitespace",
            Self::Indentation => "adjusting indentation",
        })
    }
}

/// Applies an edit and returns the edited content.
pub fn apply_edit(content: &str, edit: &EditRequest) -> Result<String, EditError> {
    match_edit(content, edit).map(|(text, _)| text)
}

/// Applies an edit and reports the matching tier. Existing line endings are kept.
pub fn match_edit(content: &str, edit: &EditRequest) -> Result<(String, EditMatch), EditError> {
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
    if old.is_empty() && !new.is_empty() {
        return Ok((format!("{new}{content}"), EditMatch::Exact));
    }
    if old == new {
        return Err(EditError::Unchanged);
    }
    let count = content.matches(old.as_str()).count();
    match count {
        0 => {}
        1 => return Ok((content.replacen(&old, &new, 1), EditMatch::Exact)),
        _ if edit.replace_all => return Ok((content.replace(&old, &new), EditMatch::Exact)),
        _ => return Err(EditError::Ambiguous { count }),
    }
    let (trailing, candidate) = whitespace_matches(content, &old, false);
    if let Some((range, _)) = candidate {
        return replace_match(content, range, new, EditMatch::TrailingWhitespace);
    }
    let (indentation, candidate) = whitespace_matches(content, &old, true);
    if let Some((range, shift)) = candidate
        && let Some(replacement) = new
            .split_inclusive('\n')
            .map(|line| shift.apply(line))
            .collect::<Option<String>>()
    {
        return replace_match(content, range, replacement, EditMatch::Indentation);
    }
    Err(EditError::NotFound {
        closest: closest_line(content, &old),
        trailing,
        indentation,
    })
}

fn replace_match(
    content: &str,
    range: std::ops::Range<usize>,
    replacement: String,
    tier: EditMatch,
) -> Result<(String, EditMatch), EditError> {
    if content[range.clone()] == replacement {
        return Err(EditError::Unchanged);
    }
    let mut edited = content.to_owned();
    edited.replace_range(range, &replacement);
    Ok((edited, tier))
}

/// An indentation shift uses literal whitespace, so tabs keep their original meaning.
#[derive(Clone, Copy, Default)]
struct IndentShift<'a> {
    remove: &'a str,
    add: &'a str,
}

impl IndentShift<'_> {
    fn apply(self, line: &str) -> Option<String> {
        if line.trim().is_empty() {
            return Some(line.to_owned());
        }
        Some(format!("{}{}", self.add, line.strip_prefix(self.remove)?))
    }

    fn matches(self, wanted: &str, actual: &str) -> bool {
        if wanted.is_empty() && actual.is_empty() {
            return true;
        }
        wanted.strip_prefix(self.remove) == actual.strip_prefix(self.add)
            && wanted.starts_with(self.remove)
    }
}

fn whitespace_matches<'a>(
    content: &'a str,
    old: &'a str,
    reindent: bool,
) -> (usize, Option<(std::ops::Range<usize>, IndentShift<'a>)>) {
    let mut offset = 0;
    let lines: Vec<_> = content
        .split_inclusive('\n')
        .map(|line| {
            let start = offset;
            offset += line.len();
            (start, line)
        })
        .collect();
    let trim = |line: &'a str| line.trim_end_matches([' ', '\t', '\r', '\n']);
    let wanted: Vec<_> = old.split_inclusive('\n').map(trim).collect();
    let Some(anchor) = wanted.iter().position(|line| !line.is_empty()) else {
        return (0, None);
    };
    let ends_in_newline = old.ends_with('\n');
    let mut count = 0;
    let mut candidate = None;
    for block in lines.windows(wanted.len()) {
        let actual_anchor = trim(block[anchor].1);
        let shift = if reindent {
            let source = &wanted[anchor]
                [..wanted[anchor].len() - wanted[anchor].trim_start_matches([' ', '\t']).len()];
            let destination = &actual_anchor
                [..actual_anchor.len() - actual_anchor.trim_start_matches([' ', '\t']).len()];
            if let Some(add) = destination.strip_suffix(source) {
                IndentShift { remove: "", add }
            } else if let Some(remove) = source.strip_suffix(destination) {
                IndentShift { remove, add: "" }
            } else {
                IndentShift {
                    remove: source,
                    add: destination,
                }
            }
        } else {
            IndentShift::default()
        };
        if !shift.matches(wanted[anchor], actual_anchor) {
            continue;
        }
        let (start, last) = block[block.len() - 1];
        if ends_in_newline && !last.ends_with('\n') {
            continue;
        }
        if wanted
            .iter()
            .zip(block)
            .all(|(wanted, (_, actual))| shift.matches(wanted, trim(actual)))
        {
            count += 1;
            if count == 1 {
                let end = if ends_in_newline {
                    last
                } else {
                    last.trim_end_matches(['\r', '\n'])
                };
                candidate = Some((block[0].0..start + end.len(), shift));
            }
        }
    }
    (count, candidate.filter(|_| count == 1))
}

/// The closest line to the first non-blank line of `sought`, for a recovery hint.
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
            let prefix = first
                .bytes()
                .zip(line.trim_start().bytes())
                .take_while(|(a, b)| a == b)
                .count();
            (shared, prefix, index, line)
        })
        .filter(|(_, _, _, line)| !line.trim().is_empty())
        .max_by_key(|(shared, prefix, index, _)| (*shared, *prefix, std::cmp::Reverse(*index)))
        .map(|(_, _, index, line)| {
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
    old_text: Option<String>,
    /// Insert new_text at the beginning of the file. Omit old_text when true.
    prepend: Option<bool>,
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

    fn argument_aliases(&self) -> &'static [(&'static str, &'static [&'static str])] {
        &[
            ("path", &["file_path", "filename"]),
            ("old_text", &["old_string"]),
            ("new_text", &["new_string"]),
        ]
    }

    fn name(&self) -> &'static str {
        "edit_file"
    }

    fn description(&self) -> &'static str {
        "Changes an existing text file. To insert at the start, send path, prepend: true, and \
         new_text including any separator newlines. To replace text, send path, old_text and \
         new_text; copy a unique block from read_file into old_text. The host may show a diff \
         for approval."
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
        let old_text = args.old_text.unwrap_or_default();
        let prepend = args.prepend.unwrap_or_default();
        if prepend && (!old_text.is_empty() || args.replace_all.unwrap_or_default()) {
            return ToolOutcome::error(
                step,
                "prepend cannot be used with old_text or replace_all.",
            );
        }
        if !prepend && old_text.is_empty() {
            return ToolOutcome::error(
                step,
                "Supply old_text for replacement or prepend: true to insert at the start. To create a file, use write_file.",
            );
        }
        if old_text.len() > MAX_EDIT_TEXT_BYTES || args.new_text.len() > MAX_EDIT_TEXT_BYTES {
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
            old_text,
            new_text: args.new_text,
            replace_all: args.replace_all.unwrap_or(false),
        }))
    }
}

#[cfg(test)]
mod tests;
