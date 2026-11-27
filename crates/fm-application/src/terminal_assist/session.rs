//! Per-terminal-tab conversation memory for the assistant (task 0336). In memory only: a session
//! lasts until its tab closes or the application exits.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash as _, Hasher as _};
use std::sync::{Mutex, MutexGuard};

use fm_agent::{Message, ToolCallRecord, fit_messages, render_text};
use fm_transport_dto::TerminalBlockDto;

use super::edit::PendingEdit;
use super::saved::{SavedSessions, SavedSummary};
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
/// Most questions and file paths remembered for a session's record.
const MAX_REMEMBERED: usize = 50;

/// What a record of a session is made from (task 0358).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SessionInfo {
    pub(crate) title: String,
    pub(crate) questions: Vec<String>,
    pub(crate) files: Vec<String>,
    pub(crate) folder: Option<String>,
    pub(crate) started: Option<String>,
    /// Whether the model answered at least once; a session without an answer is not recorded.
    pub(crate) answered: bool,
    pub(crate) messages: Vec<Message>,
}

/// Most messages queued for a running request.
pub(crate) const MAX_STEERING: usize = 8;
/// Most turns stored per session; older ones are dropped.
const MAX_TURNS: usize = 40;
/// Most command blocks remembered as already sent, per session.
const MAX_SENT_BLOCKS: usize = 200;
/// Longest stored turn, in bytes.
const MAX_TURN_BYTES: usize = 12 * 1024;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum Turn {
    Person {
        question: String,
        context: String,
    },
    /// A reply, a call's result or a note, as the agent loop recorded it.
    Agent(Message),
}

impl Turn {
    /// Whether this is one of the person's questions.
    pub(super) fn is_person(&self) -> bool {
        matches!(self, Self::Person { .. })
    }

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
    /// For the record of the session (task 0358): when it started, its folder, every question
    /// asked and the files the tools touched, kept as they happen because compaction and the turn
    /// limit drop old turns.
    started: Option<String>,
    folder: Option<String>,
    asked: Vec<String>,
    touched: Vec<String>,
    /// Whether the model answered at least once.
    answered: bool,
    /// The file waiting for approval.
    pending_write: Option<(String, PendingWrite)>,
    /// The edit waiting for approval (task 0352).
    pending_edit: Option<PendingEdit>,
    /// Edits allowed without asking: `Some(true)` for the session, `Some(false)` for the question.
    edit_grant: Option<bool>,
    /// Messages the person sent while a request was running, not yet given to the model
    /// (task 0355).
    steering: Vec<String>,
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

impl Session {
    /// A session continuing `turns` (a resumed or forked conversation), with its record's
    /// bookkeeping rebuilt from them, so its record lists what was asked before (task 0358).
    fn from_turns(turns: Vec<Turn>, folder: Option<String>) -> Self {
        let mut session = Self {
            folder,
            ..Self::default()
        };
        for turn in &turns {
            match turn {
                Turn::Person { question, .. } => session.note_question(question),
                Turn::Agent(Message::Assistant { text, calls }) => session.note_reply(text, calls),
                Turn::Agent(_) => {}
            }
        }
        session.turns = turns;
        session
    }

    fn note_question(&mut self, question: &str) {
        if self.started.is_none() {
            self.started =
                Some(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true));
        }
        if !question.trim().is_empty() && self.asked.len() < MAX_REMEMBERED {
            self.asked.push(question.to_owned());
        }
    }

    fn note_reply(&mut self, text: &str, calls: &[ToolCallRecord]) {
        if calls.is_empty() && !text.trim().is_empty() {
            self.answered = true;
        }
        for path in crate::notes::touched_paths(calls) {
            if !self.touched.iter().any(|known| known == path)
                && self.touched.len() < MAX_REMEMBERED
            {
                self.touched.push(path.to_owned());
            }
        }
    }
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
    /// Where sessions are saved (task 0354); in memory only without it.
    store: Option<SavedSessions>,
}

impl Sessions {
    /// Sessions that are also saved under `root`, so they can be resumed later.
    pub(crate) fn persistent(root: std::path::PathBuf) -> Self {
        Self {
            state: Mutex::default(),
            store: Some(SavedSessions::new(root)),
        }
    }

    /// Saves a session as it is now, titled by its first question.
    pub(super) fn save(&self, id: &str, folder: &str) {
        let Some(store) = &self.store else {
            return;
        };
        self.with(id, |session| session.folder = Some(folder.to_owned()));
        let (title, turns) = {
            let state = self.lock();
            let Some(session) = state.sessions.get(id) else {
                return;
            };
            let title = session
                .turns
                .iter()
                .find_map(|turn| match turn {
                    Turn::Person { question, .. } => Some(question.clone()),
                    Turn::Agent(_) => None,
                })
                .unwrap_or_default();
            (title, session.turns.clone())
        };
        store.save(id, folder, &title, &turns);
    }

    /// What a record of the session needs (task 0358); `None` for an unknown session.
    pub(crate) fn record_info(&self, id: &str) -> Option<SessionInfo> {
        let state = self.lock();
        let session = state.sessions.get(id)?;
        Some(SessionInfo {
            title: session.asked.first().cloned().unwrap_or_default(),
            questions: session.asked.clone(),
            files: session.touched.clone(),
            folder: session.folder.clone(),
            started: session.started.clone(),
            answered: session.answered,
            messages: session.turns.iter().map(Turn::message).collect(),
        })
    }

    /// The saved sessions, most recent first; empty without a store.
    pub(crate) fn saved(&self) -> Vec<SavedSummary> {
        self.store
            .as_ref()
            .map(SavedSessions::list)
            .unwrap_or_default()
    }

    /// Continues the saved session `from` in the tab session `into`, replacing what `into` held.
    /// The saved original is not changed. `None` when there is no such saved session.
    pub(crate) fn resume(&self, from: &str, into: &str) -> Option<SavedSummary> {
        let record = self.store.as_ref()?.load(from)?;
        let summary = SavedSummary {
            id: record.id.clone(),
            title: record.title.clone(),
            folder: record.folder.clone(),
            updated_at: record.updated_at.clone(),
            questions: record.questions,
        };
        let restored = Session::from_turns(record.turns, Some(record.folder));
        self.with(into, |session| *session = restored);
        Some(summary)
    }

    /// Deletes a saved session; `false` when there was none. A tab still showing it keeps its own
    /// copy in memory.
    pub(crate) fn delete_saved(&self, id: &str) -> bool {
        self.store.as_ref().is_some_and(|store| store.delete(id))
    }

    /// The person's questions in a session, oldest first: what `/fork` numbers. Taken from the tab's
    /// session, or from its saved copy when the tab's session is no longer in memory.
    pub(crate) fn questions(&self, id: &str) -> Vec<String> {
        self.turns(id)
            .iter()
            .filter_map(|turn| match turn {
                Turn::Person { question, .. } => Some(question.clone()),
                Turn::Agent(_) => None,
            })
            .collect()
    }

    /// Starts the tab session `into` with the history of `from` before its `question`-th question
    /// (1-based) and returns that question, for the person to ask again or change ("fork from
    /// here", task 0359). `from` is not changed. `None` when there is no such question.
    pub(crate) fn fork(&self, from: &str, question: usize, into: &str) -> Option<String> {
        let turns = self.turns(from);
        let (index, asked) = turns
            .iter()
            .enumerate()
            .filter_map(|(index, turn)| match turn {
                Turn::Person { question, .. } => Some((index, question.clone())),
                Turn::Agent(_) => None,
            })
            .nth(question.checked_sub(1)?)?;
        let folder = self
            .lock()
            .sessions
            .get(from)
            .and_then(|session| session.folder.clone());
        let restored = Session::from_turns(turns[..index].to_vec(), folder);
        self.with(into, |session| *session = restored);
        Some(asked)
    }

    /// The stored turns of a session: in memory, else saved.
    fn turns(&self, id: &str) -> Vec<Turn> {
        if let Some(session) = self.lock().sessions.get(id) {
            return session.turns.clone();
        }
        self.store
            .as_ref()
            .and_then(|store| store.load(id))
            .map(|record| record.turns)
            .unwrap_or_default()
    }

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
            // Messages meant for the previous request are not carried into this one: the
            // terminal asks what was not delivered as this question.
            session.steering.clear();
            // A new question starts with fresh limits and only a session-wide grant survives.
            session.pending_write = None;
            session.pending_edit = None;
            if session.edit_grant == Some(false) {
                session.edit_grant = None;
            }
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
            session.note_question(question);
        });
    }

    /// Holds a file until the person decides.
    pub(super) fn hold_write(&self, id: &str, folder: &str, pending: PendingWrite) {
        self.with(id, |session| {
            session.pending_write = Some((folder.to_owned(), pending))
        });
    }

    /// The held file, which is no longer held.
    pub(super) fn take_write(&self, id: &str, folder: &str) -> Option<PendingWrite> {
        let mut taken = None;
        self.with(id, |session| {
            taken = session
                .pending_write
                .take()
                .filter(|(held, _)| held == folder)
                .map(|(_, pending)| pending)
        });
        taken
    }

    /// Holds an edit until the person decides.
    pub(super) fn hold_edit(&self, id: &str, pending: PendingEdit) {
        self.with(id, |session| session.pending_edit = Some(pending));
    }

    /// The held edit, which is no longer held.
    pub(super) fn take_edit(&self, id: &str) -> Option<PendingEdit> {
        let mut taken = None;
        self.with(id, |session| taken = session.pending_edit.take());
        taken
    }

    /// Allows edits without asking for this question or the whole session.
    pub(super) fn grant_edits(&self, id: &str, whole_session: bool) {
        self.with(id, |session| session.edit_grant = Some(whole_session));
    }

    /// Whether edits may be written without asking.
    pub(super) fn edits_granted(&self, id: &str) -> bool {
        let mut granted = false;
        self.with(id, |session| granted = session.edit_grant.is_some());
        granted
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
            session.note_reply(text, calls);
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
        self.with(id, |session| {
            session.cancelled = true;
            session.steering.clear();
        });
    }

    /// Queues a message for the running request (task 0355); `false` when the queue is full.
    pub(crate) fn steer(&self, id: &str, text: &str) -> bool {
        let mut queued = false;
        self.with(id, |session| {
            if session.steering.len() < MAX_STEERING {
                session.steering.push(text.to_owned());
                queued = true;
            }
        });
        queued
    }

    /// The queued messages, now recorded in the conversation as the person's.
    pub(super) fn take_steering(&self, id: &str) -> Vec<String> {
        let mut taken = Vec::new();
        self.with(id, |session| {
            taken = std::mem::take(&mut session.steering);
            for text in &taken {
                session.turns.push(Turn::Person {
                    question: text.clone(),
                    context: String::new(),
                });
            }
        });
        taken
    }

    /// Drops what was queued but not delivered; the terminal asks it as the next question.
    pub(super) fn drop_steering(&self, id: &str) {
        self.with(id, |session| session.steering.clear());
    }

    /// Takes back one queued message before the model read it; `false` when it was already read
    /// or never queued.
    pub(super) fn withdraw_steering(&self, id: &str, text: &str) -> bool {
        let mut withdrawn = false;
        self.with(id, |session| {
            if let Some(index) = session.steering.iter().position(|queued| queued == text) {
                session.steering.remove(index);
                withdrawn = true;
            }
        });
        withdrawn
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

    /// Every message of the conversation, oldest first, one per stored turn.
    pub(super) fn messages(&self, id: &str) -> Vec<Message> {
        let state = self.lock();
        state
            .sessions
            .get(id)
            .map(|session| session.turns.iter().map(Turn::message).collect())
            .unwrap_or_default()
    }

    /// Replaces the first `covered` turns with `summary` (automatic compaction, task 0353).
    /// Nothing happens when the conversation changed meanwhile so the turns no longer line up.
    pub(super) fn replace_prefix(
        &self,
        id: &str,
        covered: usize,
        expected_len: usize,
        summary: Message,
    ) {
        self.with(id, |session| {
            if session.turns.len() == expected_len && covered <= session.turns.len() {
                session.turns.splice(..covered, [Turn::Agent(summary)]);
            }
        });
    }

    /// Replaces the conversation with `summary` (`/compact`); what waited for the person is
    /// dropped with it.
    pub(super) fn replace_with_summary(&self, id: &str, summary: &str) {
        self.with(id, |session| {
            session.pending_call = None;
            session.pending_write = None;
            session.pending_edit = None;
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
