//! Per-terminal-tab conversation memory for the assistant (task 0336). In memory only: a session
//! lasts until its tab closes or the application exits.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};

use crate::file_assistant::truncate_to_bytes;

/// Most sessions kept; the least recently used is dropped first.
pub(super) const MAX_SESSIONS: usize = 32;
/// Most bytes of conversation sent in one prompt.
pub(super) const MAX_RENDERED_BYTES: usize = 24 * 1024;
/// Most turns stored per session; older ones are dropped.
const MAX_TURNS: usize = 40;
/// Longest stored turn, in bytes.
const MAX_TURN_BYTES: usize = 12 * 1024;

#[derive(Debug, Clone)]
enum Turn {
    Person { question: String, context: String },
    Assistant(String),
    Tool(String),
}

#[derive(Debug, Default)]
struct Session {
    turns: Vec<Turn>,
    steps: u32,
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

    /// Starts a new person turn; resets the tool-step counter.
    pub(super) fn push_person(&self, id: &str, question: &str, context: &str) {
        self.with(id, |session| {
            session.steps = 0;
            session.turns.push(Turn::Person {
                question: question.to_owned(),
                context: context.to_owned(),
            });
        });
    }

    /// Records the assistant's reply.
    pub(super) fn push_assistant(&self, id: &str, text: &str) {
        self.with(id, |session| {
            session.turns.push(Turn::Assistant(
                truncate_to_bytes(text, MAX_TURN_BYTES).to_owned(),
            ));
        });
    }

    /// Records a tool result the assistant should see.
    pub(super) fn push_tool(&self, id: &str, text: &str) {
        self.with(id, |session| {
            session.turns.push(Turn::Tool(
                truncate_to_bytes(text, MAX_TURN_BYTES).to_owned(),
            ));
        });
    }

    /// Counts one tool step in the current turn and returns the new count.
    pub(super) fn next_step(&self, id: &str) -> u32 {
        let mut count = 0;
        self.with(id, |session| {
            session.steps += 1;
            count = session.steps;
        });
        count
    }

    /// The conversation as the prompt body, newest turns kept within the size bound.
    pub(super) fn render(&self, id: &str) -> String {
        let state = self.lock();
        let Some(session) = state.sessions.get(id) else {
            return String::new();
        };
        let latest_person = session
            .turns
            .iter()
            .rposition(|turn| matches!(turn, Turn::Person { .. }));
        let rendered: Vec<String> = session
            .turns
            .iter()
            .enumerate()
            .map(|(index, turn)| match turn {
                Turn::Person { question, context } => {
                    if Some(index) == latest_person && !context.is_empty() {
                        format!(
                            "Context attached by the person:\n{context}\n\nPerson: {question}\n\n"
                        )
                    } else {
                        format!("Person: {question}\n\n")
                    }
                }
                Turn::Assistant(text) => format!("Assistant: {text}\n\n"),
                Turn::Tool(text) => format!("Tool result:\n{text}\n\n"),
            })
            .collect();
        // Keep the newest whole turns that fit, and always at least the last one.
        let mut start = rendered.len();
        let mut total = 0;
        while start > 0 {
            let size = rendered[start - 1].len();
            if total + size > MAX_RENDERED_BYTES && start < rendered.len() {
                break;
            }
            total += size;
            start -= 1;
        }
        rendered[start..].concat()
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
