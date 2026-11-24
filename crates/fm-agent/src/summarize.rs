//! `summarize_file` (task 0345): a document summarised outside the conversation. The host reads the
//! whole text ([`FileTools::read_whole`]) and a fresh model call ([`SideModel`]), with no history
//! and no tools, writes the summary. Only the summary comes back, so summarising many documents
//! keeps the conversation small. A text too long for one call is summarised in parts, and the
//! part summaries are then combined.
//!
//! [`FileTools::read_whole`]: crate::FileTools::read_whole

use async_trait::async_trait;
use schemars::JsonSchema;
use serde::Deserialize;
use thiserror::Error;

use crate::tool::{ToolContext, ToolEffect, ToolOutcome, TypedTool};

/// Why a model call outside the conversation failed; shown to the model as a tool error.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
#[error("{0}")]
pub struct SideModelError(pub String);

/// A model call outside the conversation: a fresh context with no history and no tools, for tools
/// that hand work to the model without filling the conversation.
#[async_trait]
pub trait SideModel: Send + Sync {
    /// The most bytes of text one call should be given, so prompt and answer fit the model.
    fn input_bytes(&self) -> usize;
    /// The model's answer to `prompt` under `system`.
    async fn complete(&self, system: &str, prompt: &str) -> Result<String, SideModelError>;
}

/// The whole text of a file, as far as it was read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WholeText {
    /// The text, redacted by the host as for `read_file`.
    pub text: String,
    /// Whether the file goes on past what was read.
    pub truncated: bool,
}

/// Most parts one document is summarised in; a longer text is only summarised from its start.
pub const MAX_SUMMARY_PARTS: usize = 12;
/// Smallest part, whatever the host says, so a tiny window still makes progress.
const MIN_PART_BYTES: usize = 4 * 1024;

const SUMMARY_SYSTEM: &str = "You summarise one document for someone who has not read it. Be \
     factual: keep the title, authors and date when the text gives them, the main argument, the \
     findings and conclusions, and figures that matter. Add nothing that is not in the text. \
     Write plain text or simple Markdown, at most 300 words, and answer with the summary only.";

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct SummarizeFileArgs {
    /// File relative to the working folder: a text file, or a document the host converts.
    path: String,
    /// What the summary is for, so it keeps what matters (for example "the lessons on decoys").
    focus: Option<String>,
}

pub(crate) struct SummarizeFile;

#[async_trait]
impl<I: Send + 'static> TypedTool<I> for SummarizeFile {
    type Args = SummarizeFileArgs;

    fn name(&self) -> &'static str {
        "summarize_file"
    }

    fn description(&self) -> &'static str {
        "Summarises a file or document outside this conversation and returns only the summary. \
         Use it instead of read_file when you need what a document says rather than its exact \
         text, above all for long documents or several of them: the document itself never \
         enters the conversation."
    }

    fn effect(&self) -> ToolEffect {
        // Each call keeps the model busy for a while; one at a time.
        ToolEffect::Exclusive
    }

    async fn run_typed(&self, ctx: &ToolContext<'_>, args: SummarizeFileArgs) -> ToolOutcome<I> {
        let path = args.path.trim();
        let step = format!("Summarised {path}");
        // A failure says so in the step line the person sees, not only in the model's result.
        let failed = format!("Could not summarise {path}");
        if path.is_empty() {
            return ToolOutcome::error("Summarise a file", "path must not be empty.");
        }
        let (Some(files), Some(model)) = (ctx.files, ctx.side_model) else {
            return ToolOutcome::error(
                failed,
                "summarize_file is not available here; use read_file.",
            );
        };
        let part_bytes = model.input_bytes().max(MIN_PART_BYTES);
        ctx.report(format!("Reading {path}"));
        let whole = match files.read_whole(path, part_bytes * MAX_SUMMARY_PARTS).await {
            Ok(whole) => whole,
            Err(message) => return ToolOutcome::error(failed, message),
        };
        if whole.text.trim().is_empty() {
            return ToolOutcome::error(failed, format!("{path} has no text to summarise."));
        }
        let focus = args.focus.as_deref().map(str::trim).unwrap_or_default();
        match summarize(ctx, model, path, focus, &whole.text, part_bytes).await {
            Ok((summary, parts)) => {
                let read = whole.text.len().div_ceil(1024);
                let scope = if whole.truncated {
                    format!("the first {read} KB of the text; the rest was not read")
                } else {
                    format!("{read} KB of text")
                };
                let step = if parts > 1 {
                    format!("{step} ({parts} parts)")
                } else {
                    step
                };
                ToolOutcome::output(
                    step,
                    format!("Summary of {path} ({scope}):\n{}", summary.trim()),
                )
            }
            Err(error) => ToolOutcome::error(
                failed,
                format!("the model could not summarise {path}: {error}"),
            ),
        }
    }
}

/// The summary of `text` and the number of parts it was summarised in.
/// Reports each call through `ctx`, so the person sees how far a long document is.
async fn summarize(
    ctx: &ToolContext<'_>,
    model: &dyn SideModel,
    name: &str,
    focus: &str,
    text: &str,
    part_bytes: usize,
) -> Result<(String, usize), SideModelError> {
    let parts = split_text(text, part_bytes);
    let focus_line = if focus.is_empty() {
        String::new()
    } else {
        format!("Focus: {focus}\n")
    };
    if let [only] = parts.as_slice() {
        ctx.report(format!("Summarising {name}"));
        let prompt = format!("Document: {name}\n{focus_line}\n{only}");
        return Ok((model.complete(SUMMARY_SYSTEM, &prompt).await?, 1));
    }
    let count = parts.len();
    let mut summaries = Vec::with_capacity(count);
    for (index, part) in parts.iter().enumerate() {
        ctx.report(format!("Summarising {name}: part {} of {count}", index + 1));
        let prompt = format!(
            "Document: {name}, part {} of {count}\n{focus_line}Summarise this part.\n\n{part}",
            index + 1
        );
        summaries.push(model.complete(SUMMARY_SYSTEM, &prompt).await?);
    }
    let joined = summaries
        .iter()
        .enumerate()
        .map(|(index, summary)| format!("Part {}:\n{}", index + 1, summary.trim()))
        .collect::<Vec<_>>()
        .join("\n\n");
    ctx.report(format!("Summarising {name}: combining {count} parts"));
    let prompt = format!(
        "Document: {name}\n{focus_line}These are summaries of its {count} parts, in order. Write \
         one summary of the whole document.\n\n{joined}"
    );
    Ok((model.complete(SUMMARY_SYSTEM, &prompt).await?, count))
}

/// `text` in parts of at most `size` bytes, cut at a paragraph break, else a line break, else a
/// space, else a character boundary.
pub(crate) fn split_text(text: &str, size: usize) -> Vec<&str> {
    let size = size.max(1);
    let mut parts = Vec::new();
    let mut rest = text;
    while rest.len() > size {
        let mut limit = size;
        while !rest.is_char_boundary(limit) {
            limit -= 1;
        }
        if limit == 0 {
            // A size smaller than the first character: take that character alone.
            limit = rest.chars().next().map_or(rest.len(), char::len_utf8);
        }
        let window = &rest[..limit];
        // A break in the second half of the window, so parts stay reasonably even.
        let half = limit / 2;
        let cut = ["\n\n", "\n", " "]
            .iter()
            .find_map(|separator| {
                window
                    .rfind(separator)
                    .filter(|&at| at >= half)
                    .map(|at| at + separator.len())
            })
            .unwrap_or(limit);
        let (part, next) = rest.split_at(cut);
        parts.push(part);
        rest = next;
    }
    if !rest.is_empty() || parts.is_empty() {
        parts.push(rest);
    }
    parts
}

#[cfg(test)]
mod tests;
