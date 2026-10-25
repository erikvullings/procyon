//! Structured failure for desktop-only commands.
//!
//! A plain `String` rejection reaches the frontend as untranslatable English. This serialises as
//! the same `{ code, message }` shape the shared error localiser reads, so the UI shows the
//! translated sentence for known codes and wraps the raw text for platform failures.

use serde::Serialize;

/// A command failure with a stable code and the English text as fallback.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct CommandFailure {
    /// Stable machine-readable code (an `apiError` catalogue key in the frontend).
    code: &'static str,
    /// English fallback text; for `platformOperationFailed` it is the raw platform text.
    message: String,
}

impl CommandFailure {
    /// A failure with a dedicated translation.
    pub(crate) fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// A failure whose only content is raw platform or library text.
    pub(crate) fn platform(error: impl std::fmt::Display) -> Self {
        Self::new("platformOperationFailed", error.to_string())
    }
}

impl std::fmt::Display for CommandFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialises_the_code_and_message_the_frontend_localiser_reads() {
        let failure = CommandFailure::new("appUpdateUnavailable", "gone");
        assert_eq!(
            serde_json::to_value(&failure).expect("serialises"),
            serde_json::json!({ "code": "appUpdateUnavailable", "message": "gone" })
        );
        assert_eq!(CommandFailure::platform("io").to_string(), "io");
    }
}
