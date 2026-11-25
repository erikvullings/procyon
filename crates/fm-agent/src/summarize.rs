//! `summarize_file` (task 0345): a document summarised outside the conversation. The host reads the
//! whole text ([`FileTools::read_whole`]) and a fresh model call ([`SideModel`]), with no history
//! and no tools, writes the summary. Only the summary comes back, so summarising many documents
//! keeps the conversation small. A text too long for one call is summarised in parts, and the
//! part summaries are then combined in rounds that each fit one call (task 0350).
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
/// At the smallest part this is 192 KB, at the default 24 KB part about 1.1 MB (a 400-page book),
/// at the largest 4.5 MB. The part summaries are combined in rounds, so this bounds the calls and
/// the time, not what one call is given.
pub const MAX_SUMMARY_PARTS: usize = 48;
/// Smallest part, whatever the host says, so a tiny window still makes progress.
pub(crate) const MIN_PART_BYTES: usize = 4 * 1024;

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
        let report = |text: String| ctx.report(text);
        let task = SummaryTask {
            system: SUMMARY_SYSTEM,
            name: path,
            focus,
            report: &report,
        };
        match summarize(&task, model, &whole.text, part_bytes).await {
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

/// What [`summarize`] is asked: the instructions, what the text is called in the prompts, what to
/// keep, and where progress goes.
#[derive(Clone, Copy)]
pub(crate) struct SummaryTask<'a> {
    pub(crate) system: &'a str,
    pub(crate) name: &'a str,
    pub(crate) focus: &'a str,
    pub(crate) report: &'a (dyn Fn(String) + Sync),
}

/// The summary of `text` and the number of parts it was summarised in.
/// Reports each call through `ctx`, so the person sees how far a long document is.
///
/// A text longer than one call is summarised part by part. The part summaries are then combined
/// in rounds: consecutive summaries in groups that fit one call, until one summary is left, so
/// no single call is given more than `part_bytes` whatever the document's length.
pub(crate) async fn summarize(
    task: &SummaryTask<'_>,
    model: &dyn SideModel,
    text: &str,
    part_bytes: usize,
) -> Result<(String, usize), SideModelError> {
    let SummaryTask {
        system,
        name,
        focus,
        report,
    } = *task;
    let parts = split_text(text, part_bytes);
    let focus_line = if focus.is_empty() {
        String::new()
    } else {
        format!("Focus: {focus}\n")
    };
    if let [only] = parts.as_slice() {
        report(format!("Summarising {name}"));
        let prompt = format!("Document: {name}\n{focus_line}\n{only}");
        return Ok((model.complete(system, &prompt).await?, 1));
    }
    let count = parts.len();
    let mut summaries = Vec::with_capacity(count);
    for (index, part) in parts.iter().enumerate() {
        report(format!("Summarising {name}: part {} of {count}", index + 1));
        let prompt = format!(
            "Document: {name}, part {} of {count}\n{focus_line}Summarise this part.\n\n{part}",
            index + 1
        );
        let summary = model.complete(system, &prompt).await?;
        summaries.push(Covered {
            first: index + 1,
            last: index + 1,
            summary: summary.trim().to_owned(),
        });
    }
    while summaries.len() > 1 {
        let groups = group_summaries(summaries, part_bytes);
        let last_round = groups.len() == 1;
        let mut combined = Vec::with_capacity(groups.len());
        for group in groups {
            let (first, last) = (group[0].first, group[group.len() - 1].last);
            if group.len() == 1 {
                // Alone in its group: carried to the next round as it is.
                combined.extend(group);
                continue;
            }
            let joined = group
                .iter()
                .map(Covered::labelled)
                .collect::<Vec<_>>()
                .join("\n\n");
            let prompt = if last_round {
                report(format!("Summarising {name}: combining {count} parts"));
                format!(
                    "Document: {name}\n{focus_line}These are summaries of its {count} parts, in \
                     order. Write one summary of the whole document.\n\n{joined}"
                )
            } else {
                report(format!(
                    "Summarising {name}: combining parts {first}-{last} of {count}"
                ));
                format!(
                    "Document: {name}, parts {first}-{last} of {count}\n{focus_line}These are \
                     summaries of these parts, in order. Write one summary of parts \
                     {first}-{last}.\n\n{joined}"
                )
            };
            let summary = model.complete(system, &prompt).await?;
            combined.push(Covered {
                first,
                last,
                summary: summary.trim().to_owned(),
            });
        }
        summaries = combined;
    }
    let whole = summaries
        .pop()
        .map(|covered| covered.summary)
        .unwrap_or_default();
    Ok((whole, count))
}

/// A summary of parts `first` to `last` of a document.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Covered {
    first: usize,
    last: usize,
    summary: String,
}

impl Covered {
    /// The summary under a line naming the parts it covers.
    fn labelled(&self) -> String {
        if self.first == self.last {
            format!("Part {}:\n{}", self.first, self.summary)
        } else {
            format!("Parts {}-{}:\n{}", self.first, self.last, self.summary)
        }
    }
}

/// `summaries` in consecutive groups whose labelled text fits `size` bytes. A group holds at
/// least two summaries while two are left, so every round makes fewer summaries than the last,
/// even when summaries are longer than they should be.
fn group_summaries(summaries: Vec<Covered>, size: usize) -> Vec<Vec<Covered>> {
    let mut groups: Vec<Vec<Covered>> = Vec::new();
    let mut current: Vec<Covered> = Vec::new();
    let mut current_bytes = 0;
    for covered in summaries {
        let bytes = covered.labelled().len() + 2;
        if current.len() >= 2 && current_bytes + bytes > size {
            groups.push(std::mem::take(&mut current));
            current_bytes = 0;
        }
        current_bytes += bytes;
        current.push(covered);
    }
    if !current.is_empty() {
        groups.push(current);
    }
    groups
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
