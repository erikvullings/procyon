//! Cuts a streamed model reply into complete lines, so each can be shown as it arrives, while
//! lines that look like a tool call are held back: they are for the assistant, not the person.

/// Emits complete lines of a reply and holds back tool-call-looking ones (task 0336).
#[derive(Debug, Default)]
pub(crate) struct LineStreamer {
    line: String,
    held: Vec<String>,
    holding: bool,
}

impl LineStreamer {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Adds streamed text; each line it completes is passed to `emit`.
    pub(crate) fn push(&mut self, delta: &str, emit: &mut dyn FnMut(&str)) {
        // Only the new text can hold a line break: a long line is not scanned again per delta.
        let mut scan_from = self.line.len();
        self.line.push_str(delta);
        while let Some(found) = self.line[scan_from..].find('\n') {
            let end = scan_from + found;
            let line: String = self.line.drain(..=end).collect();
            self.handle(line.trim_end_matches(['\n', '\r']), emit);
            scan_from = 0;
        }
    }

    /// Passes on the last, unfinished line.
    pub(crate) fn finish(&mut self, emit: &mut dyn FnMut(&str)) {
        if !self.line.is_empty() {
            let line = std::mem::take(&mut self.line);
            self.handle(line.trim_end_matches('\r'), emit);
        }
    }

    /// Emits the held lines: the reply turned out not to be a tool call.
    pub(crate) fn release_held(&mut self, emit: &mut dyn FnMut(&str)) {
        self.holding = false;
        for line in std::mem::take(&mut self.held) {
            emit(&line);
        }
    }

    /// Drops the held lines: the reply was a tool call.
    pub(crate) fn discard_held(&mut self) {
        self.holding = false;
        self.held.clear();
    }

    fn handle(&mut self, line: &str, emit: &mut dyn FnMut(&str)) {
        if !self.holding && looks_like_tool_call(line) {
            self.holding = true;
        }
        if self.holding {
            self.held.push(line.to_owned());
        } else {
            emit(line);
        }
    }
}

/// Whether a line starts what the protocol, or a model's own habit, uses for a tool call.
fn looks_like_tool_call(line: &str) -> bool {
    let line = line.trim_start();
    line.starts_with("TOOL ")
        || line.starts_with("<tool_call")
        || line.starts_with("<|tool_call")
        || line.starts_with("<function")
        || line.starts_with("```json")
        || (line.starts_with('{') && (line.contains("\"tool\"") || line.contains("\"name\"")))
        // A call at the end of a sentence ("Let me look: {...}") is held too; a line that turns out
        // not to be one is shown after the reply.
        || (line.trim_end().ends_with('}') && line.contains("{\"tool\""))
}

#[cfg(test)]
mod tests;
