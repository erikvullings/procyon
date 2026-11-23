//! Per-terminal-tab conversation memory for the assistant (task 0336). In memory only: a session
//! lasts until its tab closes or the application exits.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash as _, Hasher as _};
use std::sync::{Mutex, MutexGuard};

use fm_agent::{Message, ToolCallRecord, fit_messages, render_text};
use fm_transport_dto::TerminalBlockDto;

use crate::file_assistant::truncate_to_bytes;

/// Most sessions kept; the least recently used is dropped first.
pub(super) const MAX_SESSIONS: usize = 32;
/// Most bytes of conversation sent in one prompt.
pub(super) const MAX_RENDERED_BYTES: usize = 24 * 1024;
/// Most conversation bytes ever sent, whatever the model's window (the endpoint's own prompt limit).
const MAX_CONVERSATION_BYTES: usize = 900 * 1024;

/// How many bytes of conversation to send for a model with `window` tokens: about half the window
/// (the rest is for the system prompt, the tools and the answer) at roughly three bytes a token.
/// An unknown window keeps the small default.
pub(super) fn conversation_budget(window: Option<u32>) -> usize {
    window.map_or(MAX_RENDERED_BYTES, |tokens| {
        (tokens as usize)
            .saturating_mul(3)
            .saturating_div(2)
            .clamp(MAX_RENDERED_BYTES / 3, MAX_CONVERSATION_BYTES)
    })
}
/// Most turns stored per session; older ones are dropped.
const MAX_TURNS: usize = 40;
/// Most command blocks remembered as already sent, per session.
const MAX_SENT_BLOCKS: usize = 200;
/// Longest stored turn, in bytes.
const MAX_TURN_BYTES: usize = 12 * 1024;

#[derive(Debug, Clone)]
enum Turn {
    Person {
        question: String,
        context: String,
    },
    /// A reply, a call's result or a note, as the agent loop recorded it.
    Agent(Message),
}

impl Turn {
    fn message(&self) -> Message {
        match self {
            Self::Person { question, context } if !context.is_empty() => Message::User(format!(
                "Context attached by the person:\n{context}\n\nPerson: {question}"
            )),
            Self::Person { question, .. } => Message::User(format!("Person: {question}")),
            Self::Agent(message) => message.clone(),
        }
    }
}

/// The result of a call that waited for the person when they asked something else instead.
const MOVED_ON: &str = "Not run: the person asked something else instead.";

/// A file the assistant wants to create, held until the person answers (task 0338).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PendingWrite {
    pub(super) name: String,
    pub(super) content: String,
}

/// What the person allowed for file creation, and for which output folder.
#[derive(Debug, Clone, PartialEq, Eq)]
struct WriteGrant {
    folder: String,
    whole_session: bool,
}

#[derive(Debug, Default)]
struct Session {
    /// The file waiting for approval.
    pending_write: Option<PendingWrite>,
    /// The call that stopped the turn for the person; what they decide is its result.
    pending_call: Option<ToolCallRecord>,
    write_grant: Option<WriteGrant>,
    /// Files and bytes created for the current question.
    written_files: usize,
    written_bytes: usize,
    turns: Vec<Turn>,
    /// Set when the person stopped the running request; ends the tool loop.
    cancelled: bool,
    /// The state parts sent so far, as they were last sent.
    state: HashMap<String, String>,
    /// Whether any state was sent yet, which decides how an update is introduced.
    state_sent: bool,
    /// Fingerprints of the command blocks already sent, oldest first.
    sent_blocks: Vec<u64>,
}

#[derive(Debug, Default)]
struct State {
    sessions: HashMap<String, Session>,
    /// Least recently used first.
    order: Vec<String>,
}

/// Holds the sessions of every terminal tab.
#[derive(Debug, Default)]
pub(crate) struct Sessions {
    state: Mutex<State>,
}

impl Sessions {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn with(&self, id: &str, action: impl FnOnce(&mut Session)) {
        let mut state = self.lock();
        state.order.retain(|existing| existing != id);
        state.order.push(id.to_owned());
        while state.order.len() > MAX_SESSIONS {
            let evicted = state.order.remove(0);
            state.sessions.remove(&evicted);
        }
        let session = state.sessions.entry(id.to_owned()).or_default();
        action(session);
        if session.turns.len() > MAX_TURNS {
            let excess = session.turns.len() - MAX_TURNS;
            session.turns.drain(..excess);
        }
    }

    #[cfg(test)]
    pub(super) fn turn_count(&self, id: &str) -> usize {
        self.lock()
            .sessions
            .get(id)
            .map_or(0, |session| session.turns.len())
    }

    /// The state parts to send with the next question: all of them the first time, afterwards
    /// only those that differ from what was last sent. A part that is no longer given is
    /// forgotten, so it is sent again if it returns. Nothing already sent is ever altered.
    pub(super) fn state_update(&self, id: &str, sections: &[(&'static str, String)]) -> String {
        let mut update = String::new();
        self.with(id, |session| {
            let first = !session.state_sent;
            let changed: Vec<&str> = sections
                .iter()
                .filter(|(key, text)| session.state.get(*key) != Some(text))
                .map(|(_, text)| text.as_str())
                .collect();
            if !changed.is_empty() {
                let label = if first {
                    "Terminal state:"
                } else {
                    "Terminal state changed:"
                };
                update = format!("{label}\n{}", changed.join("\n"));
                session.state_sent = true;
            }
            session.state = sections
                .iter()
                .map(|(key, text)| ((*key).to_owned(), text.clone()))
                .collect();
        });
        update
    }

    /// The command blocks not sent in an earlier turn of this session; they count as sent now.
    pub(super) fn unsent_blocks(
        &self,
        id: &str,
        blocks: &[TerminalBlockDto],
    ) -> Vec<TerminalBlockDto> {
        let mut unsent = Vec::new();
        self.with(id, |session| {
            for block in blocks {
                let mut hasher = DefaultHasher::new();
                (&block.command, block.exit_code, &block.output).hash(&mut hasher);
                let fingerprint = hasher.finish();
                if !session.sent_blocks.contains(&fingerprint) {
                    session.sent_blocks.push(fingerprint);
                    unsent.push(block.clone());
                }
            }
            if session.sent_blocks.len() > MAX_SENT_BLOCKS {
                let excess = session.sent_blocks.len() - MAX_SENT_BLOCKS;
                session.sent_blocks.drain(..excess);
            }
        });
        unsent
    }

    /// Starts a new person turn; clears an earlier stop.
    pub(super) fn push_person(&self, id: &str, question: &str, context: &str) {
        self.with(id, |session| {
            session.cancelled = false;
            // A new question starts with fresh limits and only a session-wide grant survives.
            session.pending_write = None;
            session.written_files = 0;
            session.written_bytes = 0;
            if session
                .write_grant
                .as_ref()
                .is_some_and(|g| !g.whole_session)
            {
                session.write_grant = None;
            }
            if let Some(call) = session.pending_call.take() {
                session
                    .turns
                    .push(Turn::Agent(Message::result(&call, MOVED_ON)));
            }
            session.turns.push(Turn::Person {
                question: question.to_owned(),
                context: context.to_owned(),
            });
        });
    }

    /// Holds a file until the person decides.
    pub(super) fn hold_write(&self, id: &str, pending: PendingWrite) {
        self.with(id, |session| session.pending_write = Some(pending));
    }

    /// The held file, which is no longer held.
    pub(super) fn take_write(&self, id: &str) -> Option<PendingWrite> {
        let mut taken = None;
        self.with(id, |session| taken = session.pending_write.take());
        taken
    }

    /// Allows creating files in `folder` for this question or the whole session.
    pub(super) fn grant_writes(&self, id: &str, folder: &str, whole_session: bool) {
        self.with(id, |session| {
            session.write_grant = Some(WriteGrant {
                folder: folder.to_owned(),
                whole_session,
            });
        });
    }

    /// Whether files may be created in `folder` without asking.
    pub(super) fn writes_granted(&self, id: &str, folder: &str) -> bool {
        let mut granted = false;
        self.with(id, |session| {
            granted = session
                .write_grant
                .as_ref()
                .is_some_and(|grant| grant.folder == folder);
        });
        granted
    }

    /// Counts a file of `bytes` against this question's limits; `false` when it would exceed them.
    pub(super) fn reserve_write(
        &self,
        id: &str,
        bytes: usize,
        max_files: usize,
        max_bytes: usize,
    ) -> bool {
        let mut allowed = false;
        self.with(id, |session| {
            if session.written_files < max_files && session.written_bytes + bytes <= max_bytes {
                session.written_files += 1;
                session.written_bytes += bytes;
                allowed = true;
            }
        });
        allowed
    }

    /// Records the assistant's reply and the calls run from it.
    pub(super) fn push_assistant(&self, id: &str, text: &str, calls: &[ToolCallRecord]) {
        self.with(id, |session| {
            session.turns.push(Turn::Agent(Message::Assistant {
                text: truncate_to_bytes(text, MAX_TURN_BYTES).to_owned(),
                calls: calls.to_vec(),
            }));
        });
    }

    /// Records the result of one call.
    pub(super) fn push_result(&self, id: &str, call: &ToolCallRecord, text: &str) {
        self.with(id, |session| {
            session.turns.push(Turn::Agent(Message::result(
                call,
                truncate_to_bytes(text, MAX_TURN_BYTES),
            )));
        });
    }

    /// Records a message from the loop to the model.
    pub(super) fn push_note(&self, id: &str, text: &str) {
        self.with(id, |session| {
            session.turns.push(Turn::Agent(Message::Note(
                truncate_to_bytes(text, MAX_TURN_BYTES).to_owned(),
            )));
        });
    }

    /// Remembers the call that stopped the turn for the person, until [`push_tool`](Self::push_tool)
    /// or a new question answers it.
    pub(super) fn hold_call(&self, id: &str, call: ToolCallRecord) {
        self.with(id, |session| session.pending_call = Some(call));
    }

    /// Records what came of the call waiting for the person (a command's output, a decline, a
    /// file created) as its result; a tool result without a call when none waits.
    pub(super) fn push_tool(&self, id: &str, text: &str) {
        self.with(id, |session| {
            let text = truncate_to_bytes(text, MAX_TURN_BYTES);
            let message = match session.pending_call.take() {
                Some(call) => Message::result(&call, text),
                None => Message::Note(format!("Tool result:\n{text}")),
            };
            session.turns.push(Turn::Agent(message));
        });
    }

    /// Counts one tool step in the current turn and returns the new count.
    /// Stops the running request of a session: its tool loop ends before the next model call.
    pub(crate) fn cancel(&self, id: &str) {
        self.with(id, |session| session.cancelled = true);
    }

    /// Whether the person stopped the running request.
    pub(super) fn is_cancelled(&self, id: &str) -> bool {
        let mut cancelled = false;
        self.with(id, |session| cancelled = session.cancelled);
        cancelled
    }

    /// The person's latest question, which a continuation after an approval still belongs to.
    pub(super) fn last_question(&self, id: &str) -> String {
        let mut question = String::new();
        self.with(id, |session| {
            if let Some(Turn::Person {
                question: asked, ..
            }) = session
                .turns
                .iter()
                .rev()
                .find(|turn| matches!(turn, Turn::Person { .. }))
            {
                question.clone_from(asked);
            }
        });
        question
    }

    /// The conversation as the prompt body, newest turns kept within the size bound.
    pub(super) fn render(&self, id: &str) -> String {
        self.render_within(id, MAX_RENDERED_BYTES)
    }

    /// Like [`render`](Self::render), within `limit` bytes.
    pub(super) fn render_within(&self, id: &str, limit: usize) -> String {
        render_text(&self.messages_within(id, limit))
    }

    /// The conversation as messages within about `limit` bytes. The current question and its
    /// context (the folder listing) always stay: without them a long run of tool results leaves
    /// the model with no task. Of the rest, the newest whole replies with their results that fit
    /// are kept, and the ones in between are left out with a note.
    pub(super) fn messages_within(&self, id: &str, limit: usize) -> Vec<Message> {
        let state = self.lock();
        let Some(session) = state.sessions.get(id) else {
            return Vec::new();
        };
        let messages: Vec<Message> = session.turns.iter().map(Turn::message).collect();
        fit_messages(&messages, limit)
    }

    /// Replaces the conversation with `summary` (`/compact`); what waited for the person is
    /// dropped with it.
    pub(super) fn replace_with_summary(&self, id: &str, summary: &str) {
        self.with(id, |session| {
            session.pending_call = None;
            session.pending_write = None;
            session.turns = vec![Turn::Agent(Message::Note(format!(
                "Summary of the conversation so far:\n{}",
                truncate_to_bytes(summary, MAX_TURN_BYTES)
            )))];
        });
    }

    /// The conversation so far, as the model sees it; empty for an unknown session. Used to turn
    /// a session into a draft note (task 0337).
    pub(crate) fn transcript(&self, id: &str) -> String {
        self.render(id)
    }

    /// Forgets a session.
    pub(crate) fn close(&self, id: &str) {
        let mut state = self.lock();
        state.sessions.remove(id);
        state.order.retain(|existing| existing != id);
    }
}

#[cfg(test)]
mod tests;
