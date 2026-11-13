//! The file assistant (task 0321): a short chat that drafts the contents of one new text file
//! through a saved LLM profile. Only the file's name and the conversation leave the host; the
//! reply is streamed back to the caller and never written to disk here.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use fm_transport_dto::{
    FileAssistantEventDto, FileAssistantFailureDto, FileAssistantMessageDto,
    FileAssistantRequestDto, FileAssistantRoleDto,
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::llm_profiles::{LlmChatGeneration, LlmProfileError, LlmProfileService, strip_reasoning};

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
    /// The profile is missing or the provider failed.
    #[error("the file assistant is unavailable")]
    Unavailable,
    /// The chat endpoint is a cloud host the person has not consented to. Consent is granted
    /// in Settings; the assistant never prompts for it.
    #[error("the file assistant's endpoint needs consent in Settings")]
    ConsentRequired,
    /// The model returned no usable text.
    #[error("the file assistant returned no text")]
    Empty,
    /// The person stopped the request.
    #[error("the file assistant request was cancelled")]
    Cancelled,
}

impl From<FileAssistantError> for FileAssistantFailureDto {
    fn from(error: FileAssistantError) -> Self {
        match error {
            FileAssistantError::InvalidRequest(_) => Self::InvalidRequest,
            FileAssistantError::Unavailable => Self::Unavailable,
            FileAssistantError::ConsentRequired => Self::ConsentRequired,
            FileAssistantError::Empty => Self::Empty,
            FileAssistantError::Cancelled => Self::Cancelled,
        }
    }
}

/// The generation capability the assistant needs. Implemented by [`LlmProfileService`]; tests
/// supply a fake.
#[async_trait]
pub(crate) trait AssistantGenerator: Send + Sync {
    async fn generate_streamed(
        &self,
        profile_id: Uuid,
        generation: LlmChatGeneration,
        cancellation: &CancellationToken,
        on_delta: &mut (dyn for<'a> FnMut(&'a str) + Send),
    ) -> Result<String, LlmProfileError>;
}

#[async_trait]
impl AssistantGenerator for LlmProfileService {
    async fn generate_streamed(
        &self,
        profile_id: Uuid,
        generation: LlmChatGeneration,
        cancellation: &CancellationToken,
        on_delta: &mut (dyn for<'a> FnMut(&'a str) + Send),
    ) -> Result<String, LlmProfileError> {
        LlmProfileService::generate_streamed(self, profile_id, generation, cancellation, on_delta)
            .await
    }
}

/// Owns the cancellation handles of requests in flight.
#[derive(Debug, Default)]
pub(crate) struct FileAssistantService {
    active: Mutex<HashMap<Uuid, CancellationToken>>,
}

impl FileAssistantService {
    /// Runs one request, reporting each fragment of the reply as it arrives and then exactly one
    /// final event.
    pub(crate) async fn stream(
        &self,
        generator: &impl AssistantGenerator,
        request: FileAssistantRequestDto,
        on_event: &mut (dyn FnMut(FileAssistantEventDto) + Send),
    ) {
        let outcome = {
            let mut forward = |fragment: &str| {
                on_event(FileAssistantEventDto::Delta {
                    text: fragment.to_owned(),
                });
            };
            self.reply(generator, request, &mut forward).await
        };
        on_event(match outcome {
            Ok(text) => FileAssistantEventDto::Done { text },
            Err(error) => FileAssistantEventDto::Failed {
                reason: error.into(),
            },
        });
    }

    /// Produces the complete reply, forwarding raw fragments to `on_delta` as they arrive.
    async fn reply(
        &self,
        generator: &impl AssistantGenerator,
        request: FileAssistantRequestDto,
        on_delta: &mut (dyn for<'a> FnMut(&'a str) + Send),
    ) -> Result<String, FileAssistantError> {
        let prompts = build_prompts(&request.file_name, &request.messages)?;
        let cancellation = CancellationToken::new();
        self.lock().insert(request.request_id, cancellation.clone());
        let mut visible = VisibleText::new(on_delta);
        let outcome = generator
            .generate_streamed(
                request.profile_id,
                LlmChatGeneration {
                    system_prompt: prompts.system,
                    user_prompt: prompts.user,
                    images: Vec::new(),
                    maximum_tokens: MAX_REPLY_TOKENS,
                    context_window: None,
                    temperature: Some(REPLY_TEMPERATURE),
                },
                &cancellation,
                &mut |fragment: &str| visible.push(fragment),
            )
            .await;
        self.lock().remove(&request.request_id);
        let text = match outcome {
            Ok(text) => text,
            Err(LlmProfileError::Cancelled) => return Err(FileAssistantError::Cancelled),
            Err(LlmProfileError::ConsentRequired(_)) => {
                return Err(FileAssistantError::ConsentRequired);
            }
            Err(_) => return Err(FileAssistantError::Unavailable),
        };
        let text = unwrap_single_fence(text.trim());
        if text.is_empty() {
            return Err(FileAssistantError::Empty);
        }
        // A text file ends with exactly one newline.
        let mut text = truncate_to_bytes(&text, MAX_REPLY_BYTES).to_owned();
        text.push('\n');
        Ok(text)
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

/// Reports only the part of a streamed reply that survives reasoning removal, so a model's hidden
/// reasoning never shows in the chat while it streams. The final text stays authoritative.
pub(crate) struct VisibleText<'a> {
    raw: String,
    sent: String,
    on_delta: &'a mut (dyn for<'b> FnMut(&'b str) + Send),
}

impl<'a> VisibleText<'a> {
    /// Forwards only the visible part of a streamed reply to `on_delta`.
    pub(crate) fn new(on_delta: &'a mut (dyn for<'b> FnMut(&'b str) + Send)) -> Self {
        Self {
            raw: String::new(),
            sent: String::new(),
            on_delta,
        }
    }

    pub(crate) fn push(&mut self, fragment: &str) {
        self.raw.push_str(fragment);
        let visible = if self.raw.contains('<') {
            Cow::Owned(strip_reasoning(&self.raw))
        } else {
            Cow::Borrowed(self.raw.as_str())
        };
        match visible.strip_prefix(self.sent.as_str()) {
            Some(rest) if !rest.is_empty() => {
                (self.on_delta)(rest);
                self.sent.push_str(rest);
            }
            _ => {}
        }
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
pub(crate) fn truncate_to_bytes(text: &str, max_bytes: usize) -> &str {
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
        /// Fragments reported before the final text, as a streaming provider would.
        Streamed(Vec<String>, String),
        Cancelled,
        Transport,
        ConsentRequired,
    }

    struct FakeGenerator {
        reply: Outcome,
        seen: StdMutex<Vec<LlmChatGeneration>>,
    }

    #[async_trait]
    impl AssistantGenerator for FakeGenerator {
        async fn generate_streamed(
            &self,
            _profile_id: Uuid,
            generation: LlmChatGeneration,
            _cancellation: &CancellationToken,
            on_delta: &mut (dyn for<'a> FnMut(&'a str) + Send),
        ) -> Result<String, LlmProfileError> {
            self.seen.lock().unwrap().push(generation);
            match &self.reply {
                Outcome::Text(text) => {
                    on_delta(text);
                    Ok(text.clone())
                }
                Outcome::Streamed(fragments, text) => {
                    for fragment in fragments {
                        on_delta(fragment);
                    }
                    Ok(text.clone())
                }
                Outcome::Cancelled => Err(LlmProfileError::Cancelled),
                Outcome::Transport => Err(LlmProfileError::Transport),
                Outcome::ConsentRequired => Err(LlmProfileError::ConsentRequired(
                    "api.example.test".to_owned(),
                )),
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

    /// Runs one request and collects every event it emits.
    async fn events(
        service: &FileAssistantService,
        generator: &FakeGenerator,
        request: FileAssistantRequestDto,
    ) -> Vec<FileAssistantEventDto> {
        let mut seen = Vec::new();
        service
            .stream(generator, request, &mut |event| seen.push(event))
            .await;
        seen
    }

    fn delta(text: &str) -> FileAssistantEventDto {
        FileAssistantEventDto::Delta { text: text.into() }
    }

    fn done(text: &str) -> FileAssistantEventDto {
        FileAssistantEventDto::Done { text: text.into() }
    }

    fn failed(reason: FileAssistantFailureDto) -> FileAssistantEventDto {
        FileAssistantEventDto::Failed { reason }
    }

    #[tokio::test]
    async fn streams_the_reply_and_sends_only_the_name_and_conversation() {
        let generator = fake(Outcome::Text("#!/usr/bin/env bash\necho hi\n".into()));
        let service = FileAssistantService::default();
        let seen = events(
            &service,
            &generator,
            request("backup.sh", "a backup script"),
        )
        .await;
        assert_eq!(
            seen,
            vec![
                delta("#!/usr/bin/env bash\necho hi\n"),
                done("#!/usr/bin/env bash\necho hi\n"),
            ]
        );
        let calls = generator.seen.lock().unwrap();
        assert!(calls[0].system_prompt.contains("backup.sh"));
        assert_eq!(calls[0].user_prompt, "Person: a backup script\n\n");
        assert!(calls[0].images.is_empty());
    }

    #[tokio::test]
    async fn hides_reasoning_from_fragments_but_not_from_the_final_text() {
        let generator = fake(Outcome::Streamed(
            vec![
                "<think>plan".into(),
                " more</think>".into(),
                "Answer".into(),
            ],
            "Answer".into(),
        ));
        let service = FileAssistantService::default();
        let seen = events(&service, &generator, request("a.txt", "x")).await;
        assert_eq!(seen, vec![delta("Answer"), done("Answer\n")]);
    }

    #[test]
    fn unwraps_a_single_code_fence_only() {
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
            assert_eq!(
                events(&service, &generator, bad).await,
                vec![failed(FileAssistantFailureDto::InvalidRequest)]
            );
        }
        let mut ends_with_assistant = request("a.txt", "x");
        ends_with_assistant.messages.push(FileAssistantMessageDto {
            role: FileAssistantRoleDto::Assistant,
            content: "y".into(),
        });
        assert_eq!(
            events(&service, &generator, ends_with_assistant).await,
            vec![failed(FileAssistantFailureDto::InvalidRequest)]
        );
        assert!(generator.seen.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn maps_provider_outcomes_to_sanitized_failures() {
        let service = FileAssistantService::default();
        assert_eq!(
            events(&service, &fake(Outcome::Cancelled), request("a.txt", "x")).await,
            vec![failed(FileAssistantFailureDto::Cancelled)]
        );
        assert_eq!(
            events(&service, &fake(Outcome::Transport), request("a.txt", "x")).await,
            vec![failed(FileAssistantFailureDto::Unavailable)]
        );
        assert_eq!(
            events(
                &service,
                &fake(Outcome::ConsentRequired),
                request("a.txt", "x")
            )
            .await,
            vec![failed(FileAssistantFailureDto::ConsentRequired)]
        );
        assert_eq!(
            events(
                &service,
                &fake(Outcome::Text("   \n".into())),
                request("a.txt", "x")
            )
            .await,
            vec![delta("   \n"), failed(FileAssistantFailureDto::Empty),]
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
