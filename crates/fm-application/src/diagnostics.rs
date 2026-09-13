//! Bounded, redacted application diagnostics shared by runtime hosts.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use fm_transport_dto::DiagnosticErrorDto;
use fm_transport_dto::redaction::redact;

const DEFAULT_CAPACITY: usize = 50;
const MAX_MESSAGE_CHARS: usize = 4_096;
const MAX_CONTEXT_CHARS: usize = 8_192;

/// Process-local, bounded diagnostics retained by either runtime host.
#[derive(Clone)]
pub struct DiagnosticErrorBuffer {
    entries: Arc<Mutex<VecDeque<DiagnosticErrorDto>>>,
    capacity: usize,
}

impl DiagnosticErrorBuffer {
    /// Creates a buffer with an explicit maximum number of entries.
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: Arc::new(Mutex::new(VecDeque::with_capacity(capacity))),
            capacity,
        }
    }

    /// Redacts and retains a frontend diagnostic, returning the safe value for structured logs.
    pub fn record_frontend(&self, error: DiagnosticErrorDto) -> DiagnosticErrorDto {
        let error = sanitize_frontend_error(error);
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if self.capacity > 0 {
            entries.push_front(error.clone());
            entries.truncate(self.capacity);
        }
        error
    }

    /// Returns retained diagnostics from newest to oldest.
    pub fn recent(&self) -> Vec<DiagnosticErrorDto> {
        self.entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .cloned()
            .collect()
    }
}

impl Default for DiagnosticErrorBuffer {
    fn default() -> Self {
        Self::new(DEFAULT_CAPACITY)
    }
}

fn sanitize_frontend_error(error: DiagnosticErrorDto) -> DiagnosticErrorDto {
    DiagnosticErrorDto {
        timestamp: truncate(error.timestamp, 64),
        message: truncate(redact(&error.message), MAX_MESSAGE_CHARS),
        code: sanitize_code(&error.code),
        context: error
            .context
            .map(|context| truncate(redact(&context), MAX_CONTEXT_CHARS)),
    }
}

fn sanitize_code(code: &str) -> String {
    let code = code
        .chars()
        .take(64)
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '_' {
                character.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect::<String>();
    if code.is_empty() {
        "FRONTEND_ERROR".to_owned()
    } else {
        code
    }
}

fn truncate(value: impl AsRef<str>, max_chars: usize) -> String {
    value.as_ref().chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::DiagnosticErrorBuffer;
    use fm_transport_dto::DiagnosticErrorDto;

    #[test]
    fn frontend_errors_are_redacted_bounded_and_returned_newest_first() {
        let buffer = DiagnosticErrorBuffer::new(2);
        for index in 0..3 {
            buffer.record_frontend(DiagnosticErrorDto {
                timestamp: format!("2026-09-13T19:36:4{index}Z"),
                message: format!(
                    "failure {index} at /Users/alice/Documents/project/file.txt token: secret-{index}"
                ),
                code: "frontend_error<script>".to_owned(),
                context: Some("/Users/alice/project/src/main.ts:10".to_owned()),
            });
        }

        let errors = buffer.recent();
        assert_eq!(errors.len(), 2);
        assert!(errors[0].message.starts_with("failure 2"));
        assert!(errors[1].message.starts_with("failure 1"));
        assert_eq!(errors[0].code, "FRONTEND_ERROR_SCRIPT_");
        assert!(!errors[0].message.contains("/Users/alice"));
        assert!(!errors[0].message.contains("secret-2"));
        assert!(
            !errors[0]
                .context
                .as_deref()
                .unwrap_or_default()
                .contains("/Users/alice")
        );
    }
}
