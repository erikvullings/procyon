//! Compaction (task 0353): when a conversation grows past most of its budget, the oldest
//! exchanges are summarised in a model call of their own ([`SideModel`]) and replaced by that
//! summary, instead of being dropped. Like Pi, it starts well before the window is full
//! ([`COMPACT_AT_PERCENT`]): near the end of a window, models get slow and lose track.
//!
//! A summary already at the start of the conversation is part of the oldest exchanges, so the
//! next compaction folds it into the new summary: summaries grow incrementally, they are never
//! rebuilt from the whole history.

use crate::message::{Message, render_text};
use crate::summarize::{SideModel, SideModelError, SummaryTask, summarize};

/// Share of the conversation budget, in percent, at which compaction starts.
pub const COMPACT_AT_PERCENT: usize = 80;
/// Share of the budget, in percent, the newest exchanges keep verbatim after compaction.
pub const KEEP_AFTER_COMPACTING_PERCENT: usize = 40;

/// How the summary is introduced to the model.
pub const COMPACTED_INTRO: &str = "Summary of the conversation so far:";

const COMPACT_SYSTEM: &str = "You summarise the earlier part of a conversation between a person \
     and an assistant that uses tools, so the assistant can continue without it. Keep what later \
     turns rely on: the person's goals, decisions and preferences; files, folders and commands \
     that were used and what came of them; findings and figures; open questions and unfinished \
     work. Drop greetings and repetition. Add nothing that is not in the text. Write plain text \
     or simple Markdown, at most 400 words, and answer with the summary only.";

/// A compaction: `summary` replaces the first `covered` messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Compaction {
    /// The summary, without [`COMPACTED_INTRO`].
    pub summary: String,
    /// How many leading messages it replaces.
    pub covered: usize,
}

impl Compaction {
    /// The message that takes the place of the covered ones.
    pub fn message(&self) -> Message {
        Message::Note(format!("{COMPACTED_INTRO}\n{}", self.summary.trim()))
    }
}

/// Whether `messages` take more than [`COMPACT_AT_PERCENT`] of `budget` bytes.
pub fn needs_compaction(messages: &[Message], budget: usize) -> bool {
    size(messages) > budget / 100 * COMPACT_AT_PERCENT
}

fn size(messages: &[Message]) -> usize {
    render_text(messages).len()
}

/// Where the kept tail starts: the earliest person message after which the rest fits in `keep`
/// bytes, so a reply is never kept without the question it answers. Falls back to the last person
/// message; `0` when nothing before it can be summarised.
pub fn compaction_cut(messages: &[Message], keep: usize) -> usize {
    let starts: Vec<usize> = messages
        .iter()
        .enumerate()
        .filter(|(_, message)| matches!(message, Message::User(_)))
        .map(|(index, _)| index)
        .collect();
    let Some(&last) = starts.last() else {
        return 0;
    };
    starts
        .iter()
        .copied()
        .find(|&start| start > 0 && size(&messages[start..]) <= keep)
        .unwrap_or(last)
}

/// Summarises the oldest messages when `messages` need it, keeping about
/// [`KEEP_AFTER_COMPACTING_PERCENT`] of `budget` verbatim. `None` when there is nothing to do.
/// `report` sees progress for a long history, which is summarised in parts.
pub async fn compact(
    messages: &[Message],
    budget: usize,
    model: &dyn SideModel,
    report: &(dyn Fn(String) + Sync),
) -> Result<Option<Compaction>, SideModelError> {
    if !needs_compaction(messages, budget) {
        return Ok(None);
    }
    let covered = compaction_cut(messages, budget / 100 * KEEP_AFTER_COMPACTING_PERCENT);
    if covered == 0 {
        return Ok(None);
    }
    let text = render_text(&messages[..covered]);
    let task = SummaryTask {
        system: COMPACT_SYSTEM,
        name: "the earlier conversation",
        focus: "",
        report,
    };
    let (summary, _) = summarize(&task, model, &text, model.input_bytes().max(4096)).await?;
    let summary = summary.trim();
    if summary.is_empty() {
        return Err(SideModelError(
            "the model wrote an empty summary".to_owned(),
        ));
    }
    Ok(Some(Compaction {
        summary: summary.to_owned(),
        covered,
    }))
}

/// A summary of a whole conversation, for a record of it (task 0358): the same instructions as a
/// compaction, so it keeps goals, decisions, files, commands and open questions; a long
/// conversation is summarised in parts and combined.
pub async fn summarize_conversation(
    messages: &[Message],
    model: &dyn SideModel,
    report: &(dyn Fn(String) + Sync),
) -> Result<String, SideModelError> {
    let text = render_text(messages);
    let task = SummaryTask {
        system: COMPACT_SYSTEM,
        name: "the conversation",
        focus: "",
        report,
    };
    let (summary, _) = summarize(&task, model, &text, model.input_bytes().max(4096)).await?;
    let summary = summary.trim();
    if summary.is_empty() {
        return Err(SideModelError(
            "the model wrote an empty summary".to_owned(),
        ));
    }
    Ok(summary.to_owned())
}

#[cfg(test)]
mod tests;
