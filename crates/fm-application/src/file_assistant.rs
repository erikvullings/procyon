//! The file assistant (task 0321): a short chat that drafts the contents of one new text file
//! through a saved LLM profile. Only the file's name and the conversation leave the host; the
//! reply is returned to the caller and never written to disk here.

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use fm_transport_dto::{
    FileAssistantMessageDto, FileAssistantReplyDto, FileAssistantRequestDto, FileAssistantRoleDto,
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::error::ApplicationError;
use crate::llm_profiles::{LlmChatGeneration, LlmProfileError, LlmProfileService};

/// Most messages kept in one request, so a long chat cannot grow without bound.
const MAX_MESSAGES: usize = 40;
/// Longest single message, in characters.
const MAX_MESSAGE_CHARS: usize = 16 * 1024;
/// Longest whole conversation sent to the model, in characters.
const MAX_CONVERSATION_CHARS: usize = 64 * 1024;
/// Longest reply returned, in bytes; the template body limit, so a draft fits a new file.
const MAX_REPLY_BYTES: usize = 64 * 1024;
/// Upper bound on generated tokens for one reply.
const MAX_REPLY_TOKENS: u32 = 4096;
/// Low randomness: the reply is file contents, not prose.
const REPLY_TEMPERATURE: f32 = 0.2;

/// Sanitized failure of one assistant request.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum FileAssistantError {
    /// The request violates a bounded contract.
    #[error("invalid file assistant request: {0}")]
    InvalidRequest(String),
    /// The profile is missing, needs consent, or the provider failed.
    #[error("the file assistant is unavailable")]
    Unavailable,
    /// The model returned no usable text.
    #[error("the file assistant returned no text")]
    Empty,
    /// The person stopped the request.
    #[error("the file assistant request was cancelled")]
    Cancelled,
}

/// The generation capability the assistant needs. Implemented by [`LlmProfileService`]; tests
/// supply a fake.
#[async_trait]
pub(crate) trait AssistantGenerator: Send + Sync {
    async fn generate(
        &self,
        profile_id: Uuid,
        generation: LlmChatGeneration,
        cancellation: &CancellationToken,
    ) -> Result<String, LlmProfileError>;
}

#[async_trait]
impl AssistantGenerator for LlmProfileService {
    async fn generate(
        &self,
        profile_id: Uuid,
        generation: LlmChatGeneration,
        cancellation: &CancellationToken,
    ) -> Result<String, LlmProfileError> {
        LlmProfileService::generate(self, profile_id, generation, cancellation).await
    }
}

/// Owns the cancellation handles of requests in flight.
#[derive(Debug, Default)]
pub(crate) struct FileAssistantService {
    active: Mutex<HashMap<Uuid, CancellationToken>>,
}

impl FileAssistantService {
    /// Produces the next assistant reply for a conversation.
    pub(crate) async fn reply(
        &self,
        generator: &impl AssistantGenerator,
        request: FileAssistantRequestDto,
    ) -> Result<FileAssistantReplyDto, FileAssistantError> {
        let prompts = build_prompts(&request.file_name, &request.messages)?;
        let cancellation = CancellationToken::new();
        self.lock().insert(request.request_id, cancellation.clone());
        let outcome = generator
            .generate(
                request.profile_id,
                LlmChatGeneration {
                    system_prompt: prompts.system,
                    user_prompt: prompts.user,
                    images: Vec::new(),
                    maximum_tokens: MAX_REPLY_TOKENS,
                    context_window: None,
                    temperature: REPLY_TEMPERATURE,
                },
                &cancellation,
            )
            .await;
        self.lock().remove(&request.request_id);
        let text = match outcome {
            Ok(text) => text,
            Err(LlmProfileError::Cancelled) => return Err(FileAssistantError::Cancelled),
            Err(_) => return Err(FileAssistantError::Unavailable),
        };
        let text = unwrap_single_fence(text.trim());
        if text.is_empty() {
            return Err(FileAssistantError::Empty);
        }
        // A text file ends with exactly one newline.
        let mut text = truncate_to_bytes(&text, MAX_REPLY_BYTES).to_owned();
        text.push('\n');
        Ok(FileAssistantReplyDto { text })
    }

    /// Cancels one running request. Returns whether it was still running.
    pub(crate) fn cancel(&self, request_id: Uuid) -> bool {
        match self.lock().remove(&request_id) {
            Some(token) => {
                token.cancel();
                true
            }
            None => false,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<Uuid, CancellationToken>> {
        self.active
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }
}

/// Maps the sanitized assistant failure onto the host-facing application error.
pub(crate) fn error_to_application(error: FileAssistantError) -> ApplicationError {
    match error {
        FileAssistantError::InvalidRequest(message) => ApplicationError::InvalidRequest(message),
        FileAssistantError::Unavailable | FileAssistantError::Empty => {
            ApplicationError::ProviderUnavailable
        }
        FileAssistantError::Cancelled => ApplicationError::OperationCancelled,
    }
}

struct Prompts {
    system: String,
    user: String,
}

/// Validates the request and renders the system instruction and the transcript.
fn build_prompts(
    file_name: &str,
    messages: &[FileAssistantMessageDto],
) -> Result<Prompts, FileAssistantError> {
    if fm_domain::location::validate_name(file_name).is_err() {
        return Err(FileAssistantError::InvalidRequest(
            "the file name is not usable".into(),
        ));
    }
    let Some(last) = messages.last() else {
        return Err(FileAssistantError::InvalidRequest(
            "the conversation is empty".into(),
        ));
    };
    if last.role != FileAssistantRoleDto::User || messages.len() > MAX_MESSAGES {
        return Err(FileAssistantError::InvalidRequest(
            "the conversation must end with a message from the person and stay bounded".into(),
        ));
    }
    let mut transcript = String::new();
    for message in messages {
        let content = message.content.trim();
        if content.is_empty() || content.chars().count() > MAX_MESSAGE_CHARS {
            return Err(FileAssistantError::InvalidRequest(
                "a message is empty or too long".into(),
            ));
        }
        let label = match message.role {
            FileAssistantRoleDto::User => "Person",
            FileAssistantRoleDto::Assistant => "Assistant",
        };
        transcript.push_str(label);
        transcript.push_str(": ");
        transcript.push_str(content);
        transcript.push_str("\n\n");
    }
    if transcript.chars().count() > MAX_CONVERSATION_CHARS {
        return Err(FileAssistantError::InvalidRequest(
            "the conversation is too long; start a new one".into(),
        ));
    }
    Ok(Prompts {
        system: format!(
            "You write the complete contents of one text file named {file_name}. Reply with the \
             file contents only: no explanation and no commentary. Do not wrap the contents in a \
             code fence. Follow the person's latest request, using the conversation for context."
        ),
        user: transcript,
    })
}

/// Removes a fence pair when the whole reply is one fenced block; otherwise returns it unchanged.
fn unwrap_single_fence(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let (Some(first), Some(last)) = (lines.first(), lines.last()) else {
        return text.to_owned();
    };
    let is_fence = |line: &str| line.trim_start().starts_with("```");
    if lines.len() >= 2
        && is_fence(first)
        && last.trim() == "```"
        && lines[1..lines.len() - 1].iter().all(|line| !is_fence(line))
    {
        return lines[1..lines.len() - 1].join("\n");
    }
    text.to_owned()
}

/// The longest prefix of `text` that fits in `max_bytes`, cut on a character boundary.
fn truncate_to_bytes(text: &str, max_bytes: usize) -> &str {
    if text.len() <= max_bytes {
        return text;
    }
    let mut end = max_bytes;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex as StdMutex;

    use super::*;

    /// Canned provider outcome; `LlmProfileError` is not `Clone`, so tests name the variant.
    #[derive(Clone)]
    enum Outcome {
        Text(String),
        Cancelled,
        Transport,
    }

    struct FakeGenerator {
        reply: Outcome,
        seen: StdMutex<Vec<LlmChatGeneration>>,
    }

    #[async_trait]
    impl AssistantGenerator for FakeGenerator {
        async fn generate(
            &self,
            _profile_id: Uuid,
            generation: LlmChatGeneration,
            _cancellation: &CancellationToken,
        ) -> Result<String, LlmProfileError> {
            self.seen.lock().unwrap().push(generation);
            match &self.reply {
                Outcome::Text(text) => Ok(text.clone()),
                Outcome::Cancelled => Err(LlmProfileError::Cancelled),
                Outcome::Transport => Err(LlmProfileError::Transport),
            }
        }
    }

    fn fake(reply: Outcome) -> FakeGenerator {
        FakeGenerator {
            reply,
            seen: StdMutex::new(Vec::new()),
        }
    }

    fn request(file_name: &str, content: &str) -> FileAssistantRequestDto {
        FileAssistantRequestDto {
            request_id: Uuid::new_v4(),
            profile_id: Uuid::new_v4(),
            file_name: file_name.into(),
            messages: vec![FileAssistantMessageDto {
                role: FileAssistantRoleDto::User,
                content: content.into(),
            }],
        }
    }

    #[tokio::test]
    async fn returns_the_reply_and_sends_only_the_name_and_conversation() {
        let generator = fake(Outcome::Text("#!/usr/bin/env bash\necho hi\n".into()));
        let service = FileAssistantService::default();
        let reply = service
            .reply(&generator, request("backup.sh", "a backup script"))
            .await
            .expect("reply");
        assert_eq!(reply.text, "#!/usr/bin/env bash\necho hi\n");
        let seen = generator.seen.lock().unwrap();
        assert!(seen[0].system_prompt.contains("backup.sh"));
        assert_eq!(seen[0].user_prompt, "Person: a backup script\n\n");
        assert!(seen[0].images.is_empty());
    }

    #[tokio::test]
    async fn unwraps_a_single_code_fence_only() {
        assert_eq!(unwrap_single_fence("```bash\necho hi\n```"), "echo hi");
        assert_eq!(
            unwrap_single_fence("Here:\n```bash\necho hi\n```"),
            "Here:\n```bash\necho hi\n```"
        );
        assert_eq!(
            unwrap_single_fence("```\na\n```\n```\nb\n```"),
            "```\na\n```\n```\nb\n```"
        );
    }

    #[tokio::test]
    async fn rejects_unsafe_names_and_bad_conversations() {
        let service = FileAssistantService::default();
        let generator = fake(Outcome::Text("x".into()));
        for bad in [request("../evil.sh", "x"), request("", "x")] {
            assert!(matches!(
                service.reply(&generator, bad).await,
                Err(FileAssistantError::InvalidRequest(_))
            ));
        }
        let mut ends_with_assistant = request("a.txt", "x");
        ends_with_assistant.messages.push(FileAssistantMessageDto {
            role: FileAssistantRoleDto::Assistant,
            content: "y".into(),
        });
        assert!(matches!(
            service.reply(&generator, ends_with_assistant).await,
            Err(FileAssistantError::InvalidRequest(_))
        ));
        assert!(generator.seen.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn maps_provider_outcomes_to_sanitized_errors() {
        let service = FileAssistantService::default();
        let cancelled = fake(Outcome::Cancelled);
        assert_eq!(
            service.reply(&cancelled, request("a.txt", "x")).await,
            Err(FileAssistantError::Cancelled)
        );
        let failing = fake(Outcome::Transport);
        assert_eq!(
            service.reply(&failing, request("a.txt", "x")).await,
            Err(FileAssistantError::Unavailable)
        );
        let blank = fake(Outcome::Text("   \n".into()));
        assert_eq!(
            service.reply(&blank, request("a.txt", "x")).await,
            Err(FileAssistantError::Empty)
        );
    }

    #[test]
    fn cancelling_an_unknown_request_reports_false() {
        assert!(!FileAssistantService::default().cancel(Uuid::new_v4()));
    }

    #[test]
    fn truncation_keeps_a_valid_character_boundary() {
        assert_eq!(truncate_to_bytes("ééé", 3), "é");
        assert_eq!(truncate_to_bytes("abc", 10), "abc");
    }
}
