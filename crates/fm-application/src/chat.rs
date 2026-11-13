//! Chat replies (task 0324): one answer in a saved conversation, through the chat role (0322).
//! The person's message is saved before the model is asked, and the reply is saved only when it
//! completes. The model receives the conversation and any read-only context the person attached.
//! Nothing here can write to disk; the model may only answer in text.

use std::collections::HashMap;
use std::sync::Mutex;

use std::io::Cursor;

use async_trait::async_trait;
use base64::Engine as _;
use fm_transport_dto::{
    CancelChatReplyRequestDto, ChatFailureDto, ChatImageDto, ChatReplyEventDto, ChatReplyRequestDto,
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::chat_conversations::{
    ChatConversation, ChatConversationError, ChatConversationService, ChatMessage, ChatRole,
};
use crate::file_assistant::{VisibleText, truncate_to_bytes};
use crate::llm_profiles::{
    LlmChatGeneration, LlmImageAttachment, LlmProfileError, LlmProfileService, LlmRole,
};

/// Longest message the person sends, in characters.
const MAX_MESSAGE_CHARS: usize = 16 * 1024;
/// Longest context attached to one message, in characters.
const MAX_CONTEXT_CHARS: usize = 32 * 1024;
/// Most transcript bytes sent with one request; the oldest messages drop out first.
const MAX_PROMPT_BYTES: usize = 96 * 1024;
/// Longest reply kept, in bytes.
const MAX_REPLY_BYTES: usize = 64 * 1024;
/// Most images sent with one message.
const MAX_IMAGES: usize = 4;
/// Longest side of an image, in pixels. The client shrinks pasted images to fit; larger ones are
/// refused rather than cut.
const MAX_IMAGE_EDGE: u32 = 1024;
/// Most decoded image bytes sent with one message.
const MAX_IMAGE_BYTES: usize = 4 * 1024 * 1024;
/// Upper bound on generated tokens for one reply.
const MAX_REPLY_TOKENS: u32 = 4096;

const SYSTEM_PROMPT: &str = "You are the chat assistant inside Procyon, a file manager. Answer the \
     person's questions helpfully and concisely. Some messages include context the person attached \
     from their folders (names, sizes, dates or file contents). Use it when it is relevant, and say \
     when it does not contain the answer. You cannot change files or run commands; when an action \
     would help, describe it in text for the person to carry out themselves.";

/// Why a reply ended without text.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum ChatReplyError {
    /// The request violates a bounded contract.
    #[error("invalid chat request: {0}")]
    InvalidRequest(&'static str),
    /// The conversation does not exist.
    #[error("the chat conversation does not exist")]
    NotFound,
    /// No endpoint is assigned to the chat role.
    #[error("no chat endpoint is assigned")]
    RoleNotAssigned,
    /// The chat endpoint is a cloud host the person has not consented to. Consent is granted in
    /// Settings; the chat never prompts for it.
    #[error("the chat endpoint needs consent in Settings")]
    ConsentRequired,
    /// The provider or the save failed.
    #[error("the chat is unavailable")]
    Unavailable,
    /// The model returned no usable text.
    #[error("the chat returned no text")]
    Empty,
    /// The person stopped the reply.
    #[error("the chat reply was cancelled")]
    Cancelled,
    /// The chat endpoint is not marked as accepting images.
    #[error("the chat endpoint does not accept images")]
    ImagesNotSupported,
}

impl From<ChatReplyError> for ChatFailureDto {
    fn from(error: ChatReplyError) -> Self {
        match error {
            ChatReplyError::InvalidRequest(_) => Self::InvalidRequest,
            ChatReplyError::NotFound => Self::NotFound,
            ChatReplyError::RoleNotAssigned => Self::RoleNotAssigned,
            ChatReplyError::ConsentRequired => Self::ConsentRequired,
            ChatReplyError::Unavailable => Self::Unavailable,
            ChatReplyError::Empty => Self::Empty,
            ChatReplyError::Cancelled => Self::Cancelled,
            ChatReplyError::ImagesNotSupported => Self::ImagesNotSupported,
        }
    }
}

impl From<ChatConversationError> for ChatReplyError {
    fn from(error: ChatConversationError) -> Self {
        match error {
            ChatConversationError::NotFound => Self::NotFound,
            ChatConversationError::Invalid(message) => Self::InvalidRequest(message),
            ChatConversationError::Persistence => Self::Unavailable,
        }
    }
}

/// The generation capability the chat needs. Implemented by [`LlmProfileService`]; tests supply a
/// fake.
#[async_trait]
pub(crate) trait ChatGenerator: Send + Sync {
    /// Whether the role's model accepts images.
    async fn accepts_images(&self, role: LlmRole) -> Result<bool, LlmProfileError>;

    async fn generate_for_role(
        &self,
        role: LlmRole,
        generation: LlmChatGeneration,
        cancellation: &CancellationToken,
        on_delta: &mut (dyn for<'a> FnMut(&'a str) + Send),
    ) -> Result<String, LlmProfileError>;
}

#[async_trait]
impl ChatGenerator for LlmProfileService {
    async fn accepts_images(&self, role: LlmRole) -> Result<bool, LlmProfileError> {
        self.role_accepts_images(role).await
    }

    async fn generate_for_role(
        &self,
        role: LlmRole,
        generation: LlmChatGeneration,
        cancellation: &CancellationToken,
        on_delta: &mut (dyn for<'a> FnMut(&'a str) + Send),
    ) -> Result<String, LlmProfileError> {
        self.generate_role_streamed(role, generation, cancellation, on_delta)
            .await
    }
}

/// Owns the cancellation handles of replies in flight.
#[derive(Debug, Default)]
pub(crate) struct ChatService {
    active: Mutex<HashMap<Uuid, CancellationToken>>,
}

impl ChatService {
    /// Runs one reply, reporting fragments as they arrive and then exactly one final event.
    pub(crate) async fn stream(
        &self,
        generator: &impl ChatGenerator,
        conversations: &ChatConversationService,
        request: ChatReplyRequestDto,
        on_event: &mut (dyn FnMut(ChatReplyEventDto) + Send),
    ) {
        let outcome = {
            let mut forward = |fragment: &str| {
                on_event(ChatReplyEventDto::Delta {
                    text: fragment.to_owned(),
                });
            };
            self.reply(generator, conversations, request, &mut forward)
                .await
        };
        on_event(match outcome {
            Ok(text) => ChatReplyEventDto::Done { text },
            Err(error) => ChatReplyEventDto::Failed {
                reason: error.into(),
            },
        });
    }

    async fn reply(
        &self,
        generator: &impl ChatGenerator,
        conversations: &ChatConversationService,
        request: ChatReplyRequestDto,
        on_delta: &mut (dyn for<'a> FnMut(&'a str) + Send),
    ) -> Result<String, ChatReplyError> {
        let images = validate_images(&request.images)?;
        let (content, context) = if request.retry {
            // The person's message is already saved; it is asked again, not saved twice.
            let conversation = conversations.get(request.workspace_id, request.conversation_id)?;
            conversation
                .messages
                .last()
                .filter(|message| message.role == ChatRole::User)
                .map(|message| (message.content.clone(), message.context.clone()))
                .ok_or(ChatReplyError::InvalidRequest(
                    "there is no message to retry",
                ))?
        } else {
            validate_message(&request.content, request.context)?
        };
        if !images.is_empty() {
            let accepted = generator
                .accepts_images(LlmRole::Chat)
                .await
                .map_err(|error| match error {
                    LlmProfileError::RoleNotAssigned => ChatReplyError::RoleNotAssigned,
                    _ => ChatReplyError::Unavailable,
                })?;
            if !accepted {
                return Err(ChatReplyError::ImagesNotSupported);
            }
        }
        let saved = if request.retry {
            conversations.get(request.workspace_id, request.conversation_id)?
        } else {
            conversations.append(
                request.workspace_id,
                request.conversation_id,
                ChatMessage {
                    role: ChatRole::User,
                    content,
                    context,
                },
            )?
        };
        let prompts = build_prompts(&saved);
        let cancellation = CancellationToken::new();
        self.lock().insert(request.request_id, cancellation.clone());
        let mut visible = VisibleText::new(on_delta);
        let outcome = generator
            .generate_for_role(
                LlmRole::Chat,
                LlmChatGeneration {
                    system_prompt: SYSTEM_PROMPT.to_owned(),
                    user_prompt: prompts,
                    images,
                    maximum_tokens: MAX_REPLY_TOKENS,
                    context_window: None,
                    temperature: None,
                },
                &cancellation,
                &mut |fragment: &str| visible.push(fragment),
            )
            .await;
        self.lock().remove(&request.request_id);
        let text = match outcome {
            Ok(text) => text,
            Err(LlmProfileError::Cancelled) => return Err(ChatReplyError::Cancelled),
            Err(LlmProfileError::ConsentRequired(_)) => {
                return Err(ChatReplyError::ConsentRequired);
            }
            Err(LlmProfileError::RoleNotAssigned) => return Err(ChatReplyError::RoleNotAssigned),
            Err(_) => return Err(ChatReplyError::Unavailable),
        };
        let text = truncate_to_bytes(text.trim(), MAX_REPLY_BYTES).trim_end();
        if text.is_empty() {
            return Err(ChatReplyError::Empty);
        }
        conversations.append(
            request.workspace_id,
            request.conversation_id,
            ChatMessage {
                role: ChatRole::Assistant,
                content: text.to_owned(),
                context: None,
            },
        )?;
        Ok(text.to_owned())
    }

    /// Stops one running reply. Returns whether it was still running. The person's message stays
    /// saved; no reply is saved.
    pub(crate) fn cancel(&self, request: CancelChatReplyRequestDto) -> bool {
        match self.lock().remove(&request.request_id) {
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

/// Decodes the pasted images. Each must be a PNG, JPEG or WebP file within the size limits; a
/// violation refuses the whole message rather than dropping an image.
fn validate_images(images: &[ChatImageDto]) -> Result<Vec<LlmImageAttachment>, ChatReplyError> {
    if images.len() > MAX_IMAGES {
        return Err(ChatReplyError::InvalidRequest("too many images"));
    }
    let mut total = 0usize;
    let mut attachments = Vec::with_capacity(images.len());
    for image in images {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(image.data.as_bytes())
            .map_err(|_| ChatReplyError::InvalidRequest("an image is not valid base64"))?;
        total = total.saturating_add(bytes.len());
        if total > MAX_IMAGE_BYTES {
            return Err(ChatReplyError::InvalidRequest("the images are too large"));
        }
        let reader = image::ImageReader::new(Cursor::new(bytes.as_slice()))
            .with_guessed_format()
            .map_err(|_| ChatReplyError::InvalidRequest("an image is not a readable file"))?;
        let media_type = match reader.format() {
            Some(image::ImageFormat::Png) => "image/png",
            Some(image::ImageFormat::Jpeg) => "image/jpeg",
            Some(image::ImageFormat::WebP) => "image/webp",
            _ => {
                return Err(ChatReplyError::InvalidRequest(
                    "images must be PNG, JPEG or WebP",
                ));
            }
        };
        let (width, height) = reader
            .into_dimensions()
            .map_err(|_| ChatReplyError::InvalidRequest("an image is not a readable file"))?;
        if width.max(height) > MAX_IMAGE_EDGE {
            return Err(ChatReplyError::InvalidRequest(
                "an image is larger than 1024 pixels",
            ));
        }
        attachments.push(LlmImageAttachment {
            media_type: media_type.to_owned(),
            data: bytes,
        });
    }
    Ok(attachments)
}

/// Checks the message and its attached context, and returns them as they will be saved.
fn validate_message(
    content: &str,
    context: Option<String>,
) -> Result<(String, Option<String>), ChatReplyError> {
    let content = content.trim();
    if content.is_empty() {
        return Err(ChatReplyError::InvalidRequest("the message is empty"));
    }
    if content.chars().count() > MAX_MESSAGE_CHARS {
        return Err(ChatReplyError::InvalidRequest("the message is too long"));
    }
    let context = context.filter(|text| !text.trim().is_empty());
    if context
        .as_deref()
        .is_some_and(|text| text.chars().count() > MAX_CONTEXT_CHARS)
    {
        return Err(ChatReplyError::InvalidRequest(
            "the attached context is too long",
        ));
    }
    Ok((content.to_owned(), context))
}

/// Renders the conversation for the model, newest messages first in the budget. The latest
/// message is always included.
fn build_prompts(conversation: &ChatConversation) -> String {
    let mut sections: Vec<String> = Vec::new();
    let mut total = 0;
    for message in conversation.messages.iter().rev() {
        let section = render(message);
        if total + section.len() > MAX_PROMPT_BYTES && !sections.is_empty() {
            break;
        }
        total += section.len();
        sections.push(section);
    }
    sections.reverse();
    sections.concat()
}

fn render(message: &ChatMessage) -> String {
    let label = match message.role {
        ChatRole::User => "Person",
        ChatRole::Assistant => "Assistant",
    };
    let context = message
        .context
        .as_deref()
        .map(|text| format!("Context attached by the person:\n{text}\n\n"))
        .unwrap_or_default();
    format!("{context}{label}: {}\n\n", message.content)
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex as StdMutex;

    use fm_settings::SettingsStore;
    use tempfile::tempdir;

    use super::*;

    #[derive(Clone)]
    enum Outcome {
        Text(String),
        ConsentRequired,
        RoleNotAssigned,
        Transport,
    }

    /// Records every generation request and the role it was made for.
    struct FakeGenerator {
        reply: Outcome,
        accepts_images: bool,
        seen: StdMutex<Vec<(LlmRole, LlmChatGeneration)>>,
    }

    #[async_trait]
    impl ChatGenerator for FakeGenerator {
        async fn accepts_images(&self, _role: LlmRole) -> Result<bool, LlmProfileError> {
            Ok(self.accepts_images)
        }

        async fn generate_for_role(
            &self,
            role: LlmRole,
            generation: LlmChatGeneration,
            _cancellation: &CancellationToken,
            on_delta: &mut (dyn for<'a> FnMut(&'a str) + Send),
        ) -> Result<String, LlmProfileError> {
            self.seen.lock().unwrap().push((role, generation));
            match &self.reply {
                Outcome::Text(text) => {
                    on_delta(text);
                    Ok(text.clone())
                }
                Outcome::ConsentRequired => {
                    Err(LlmProfileError::ConsentRequired("api.example.test".into()))
                }
                Outcome::RoleNotAssigned => Err(LlmProfileError::RoleNotAssigned),
                Outcome::Transport => Err(LlmProfileError::Transport),
            }
        }
    }

    fn fake(reply: Outcome) -> FakeGenerator {
        FakeGenerator {
            reply,
            accepts_images: true,
            seen: StdMutex::new(Vec::new()),
        }
    }

    fn png(width: u32, height: u32) -> String {
        let mut bytes = Vec::new();
        image::RgbImage::new(width, height)
            .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
            .unwrap();
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    fn with_images(mut request: ChatReplyRequestDto, images: &[String]) -> ChatReplyRequestDto {
        request.images = images
            .iter()
            .map(|data| ChatImageDto { data: data.clone() })
            .collect();
        request
    }

    fn store() -> (tempfile::TempDir, ChatConversationService) {
        let directory = tempdir().unwrap();
        let service = ChatConversationService::new(&SettingsStore::new(directory.path())).unwrap();
        (directory, service)
    }

    fn request(
        workspace: Uuid,
        conversation: Uuid,
        content: &str,
        context: Option<&str>,
    ) -> ChatReplyRequestDto {
        ChatReplyRequestDto {
            request_id: Uuid::new_v4(),
            workspace_id: workspace,
            conversation_id: conversation,
            content: content.into(),
            context: context.map(str::to_owned),
            images: Vec::new(),
            retry: false,
        }
    }

    async fn run(
        service: &ChatService,
        generator: &FakeGenerator,
        conversations: &ChatConversationService,
        request: ChatReplyRequestDto,
    ) -> Vec<ChatReplyEventDto> {
        let mut seen = Vec::new();
        service
            .stream(generator, conversations, request, &mut |event| {
                seen.push(event)
            })
            .await;
        seen
    }

    fn failed(reason: ChatFailureDto) -> ChatReplyEventDto {
        ChatReplyEventDto::Failed { reason }
    }

    #[tokio::test]
    async fn saves_the_person_and_the_reply_and_uses_the_chat_role() {
        let (_directory, conversations) = store();
        let workspace = Uuid::new_v4();
        let id = conversations.create(workspace).unwrap().id;
        let generator = fake(Outcome::Text("Hello there.".into()));
        let events = run(
            &ChatService::default(),
            &generator,
            &conversations,
            request(workspace, id, "Hi", None),
        )
        .await;
        assert_eq!(
            events.last(),
            Some(&ChatReplyEventDto::Done {
                text: "Hello there.".into()
            })
        );
        let saved = conversations.get(workspace, id).unwrap();
        let contents: Vec<(&ChatRole, &str)> = saved
            .messages
            .iter()
            .map(|message| (&message.role, message.content.as_str()))
            .collect();
        assert_eq!(
            contents,
            vec![
                (&ChatRole::User, "Hi"),
                (&ChatRole::Assistant, "Hello there."),
            ]
        );
        assert_eq!(generator.seen.lock().unwrap()[0].0, LlmRole::Chat);
    }

    #[tokio::test]
    async fn sends_earlier_turns_and_the_attached_context_exactly() {
        let (_directory, conversations) = store();
        let workspace = Uuid::new_v4();
        let id = conversations.create(workspace).unwrap().id;
        let service = ChatService::default();
        run(
            &service,
            &fake(Outcome::Text("First answer".into())),
            &conversations,
            request(workspace, id, "First question", None),
        )
        .await;
        let generator = fake(Outcome::Text("Second answer".into()));
        let listing = "Folder listing of /notes (1 entry):\n- todo.md (1.2 KB)";
        run(
            &service,
            &generator,
            &conversations,
            request(workspace, id, "What is in it?", Some(listing)),
        )
        .await;
        let calls = generator.seen.lock().unwrap();
        let prompt = &calls[0].1.user_prompt;
        assert_eq!(
            prompt,
            &format!(
                "Person: First question\n\nAssistant: First answer\n\n\
                 Context attached by the person:\n{listing}\n\nPerson: What is in it?\n\n"
            )
        );
    }

    #[tokio::test]
    async fn stop_cancels_a_running_reply_and_keeps_only_the_person_message() {
        let (_directory, conversations) = store();
        let workspace = Uuid::new_v4();
        let id = conversations.create(workspace).unwrap().id;
        let service = ChatService::default();
        let started = tokio::sync::Notify::new();
        let generator = BlockingGenerator { started: &started };
        let req = request(workspace, id, "Long question", None);
        let request_id = req.request_id;
        let run = async {
            let mut seen = Vec::new();
            service
                .stream(&generator, &conversations, req, &mut |event| {
                    seen.push(event)
                })
                .await;
            seen
        };
        let stop = async {
            started.notified().await;
            assert!(service.cancel(CancelChatReplyRequestDto { request_id }));
        };
        let ((), events) = tokio::join!(stop, run);
        assert_eq!(events, vec![failed(ChatFailureDto::Cancelled)]);
        let saved = conversations.get(workspace, id).unwrap();
        assert_eq!(saved.messages.len(), 1);
        assert!(!service.cancel(CancelChatReplyRequestDto { request_id }));
    }

    /// Waits until it is cancelled, the way a streaming provider does.
    struct BlockingGenerator<'a> {
        started: &'a tokio::sync::Notify,
    }

    #[async_trait]
    impl ChatGenerator for BlockingGenerator<'_> {
        async fn accepts_images(&self, _role: LlmRole) -> Result<bool, LlmProfileError> {
            Ok(true)
        }

        async fn generate_for_role(
            &self,
            _role: LlmRole,
            _generation: LlmChatGeneration,
            cancellation: &CancellationToken,
            _on_delta: &mut (dyn for<'a> FnMut(&'a str) + Send),
        ) -> Result<String, LlmProfileError> {
            self.started.notify_one();
            cancellation.cancelled().await;
            Err(LlmProfileError::Cancelled)
        }
    }

    #[tokio::test]
    async fn missing_consent_and_missing_role_are_reported_not_prompted() {
        let (_directory, conversations) = store();
        let workspace = Uuid::new_v4();
        let id = conversations.create(workspace).unwrap().id;
        let service = ChatService::default();
        assert_eq!(
            run(
                &service,
                &fake(Outcome::ConsentRequired),
                &conversations,
                request(workspace, id, "Hi", None),
            )
            .await,
            vec![failed(ChatFailureDto::ConsentRequired)]
        );
        assert_eq!(
            run(
                &service,
                &fake(Outcome::RoleNotAssigned),
                &conversations,
                request(workspace, id, "Hi", None),
            )
            .await,
            vec![failed(ChatFailureDto::RoleNotAssigned)]
        );
        assert_eq!(
            run(
                &service,
                &fake(Outcome::Transport),
                &conversations,
                request(workspace, id, "Hi", None),
            )
            .await,
            vec![failed(ChatFailureDto::Unavailable)]
        );
        // The person's messages are kept; no reply was saved for any of the failures.
        assert_eq!(conversations.get(workspace, id).unwrap().messages.len(), 3);
    }

    #[tokio::test]
    async fn unknown_conversations_and_bad_input_never_reach_the_model() {
        let (_directory, conversations) = store();
        let workspace = Uuid::new_v4();
        let id = conversations.create(workspace).unwrap().id;
        let service = ChatService::default();
        let generator = fake(Outcome::Text("x".into()));
        assert_eq!(
            run(
                &service,
                &generator,
                &conversations,
                request(workspace, Uuid::new_v4(), "Hi", None),
            )
            .await,
            vec![failed(ChatFailureDto::NotFound)]
        );
        assert_eq!(
            run(
                &service,
                &generator,
                &conversations,
                request(workspace, id, "   ", None)
            )
            .await,
            vec![failed(ChatFailureDto::InvalidRequest)]
        );
        let too_much_context = "c".repeat(MAX_CONTEXT_CHARS + 1);
        assert_eq!(
            run(
                &service,
                &generator,
                &conversations,
                request(workspace, id, "Hi", Some(&too_much_context)),
            )
            .await,
            vec![failed(ChatFailureDto::InvalidRequest)]
        );
        assert!(generator.seen.lock().unwrap().is_empty());
        assert!(
            conversations
                .get(workspace, id)
                .unwrap()
                .messages
                .is_empty()
        );
    }

    #[tokio::test]
    async fn a_blank_reply_is_an_empty_failure_and_is_not_saved() {
        let (_directory, conversations) = store();
        let workspace = Uuid::new_v4();
        let id = conversations.create(workspace).unwrap().id;
        let events = run(
            &ChatService::default(),
            &fake(Outcome::Text("  \n".into())),
            &conversations,
            request(workspace, id, "Hi", None),
        )
        .await;
        assert_eq!(events.last(), Some(&failed(ChatFailureDto::Empty)));
        assert_eq!(conversations.get(workspace, id).unwrap().messages.len(), 1);
    }

    #[test]
    fn long_histories_drop_the_oldest_messages_first() {
        let mut messages = Vec::new();
        for index in 0..40 {
            messages.push(ChatMessage {
                role: ChatRole::User,
                content: format!("{index}:{}", "p".repeat(4 * 1024)),
                context: None,
            });
        }
        let conversation = ChatConversation {
            id: Uuid::new_v4(),
            title: String::new(),
            updated_at: String::new(),
            messages,
        };
        let prompt = build_prompts(&conversation);
        assert!(prompt.len() <= MAX_PROMPT_BYTES);
        assert!(prompt.ends_with(&format!("Person: 39:{}\n\n", "p".repeat(4 * 1024))));
        assert!(!prompt.contains("Person: 0:"));
    }

    #[tokio::test]
    async fn sends_pasted_images_with_the_message_only_to_an_accepting_endpoint() {
        let (_directory, conversations) = store();
        let workspace = Uuid::new_v4();
        let id = conversations.create(workspace).unwrap().id;
        let generator = fake(Outcome::Text("A square.".into()));
        let events = run(
            &ChatService::default(),
            &generator,
            &conversations,
            with_images(request(workspace, id, "What is this?", None), &[png(8, 8)]),
        )
        .await;
        assert_eq!(
            events.last(),
            Some(&ChatReplyEventDto::Done {
                text: "A square.".into()
            })
        );
        let seen = generator.seen.lock().unwrap();
        assert_eq!(seen[0].1.images.len(), 1);
        assert_eq!(seen[0].1.images[0].media_type, "image/png");
        assert_eq!(&seen[0].1.images[0].data[..4], b"\x89PNG");
    }

    #[tokio::test]
    async fn refuses_images_for_an_endpoint_not_marked_as_accepting_them() {
        let (_directory, conversations) = store();
        let workspace = Uuid::new_v4();
        let id = conversations.create(workspace).unwrap().id;
        let generator = FakeGenerator {
            accepts_images: false,
            ..fake(Outcome::Text("unused".into()))
        };
        let events = run(
            &ChatService::default(),
            &generator,
            &conversations,
            with_images(request(workspace, id, "Look", None), &[png(8, 8)]),
        )
        .await;
        assert_eq!(events, vec![failed(ChatFailureDto::ImagesNotSupported)]);
        assert!(generator.seen.lock().unwrap().is_empty());
        assert!(
            conversations
                .get(workspace, id)
                .unwrap()
                .messages
                .is_empty()
        );
    }

    #[test]
    fn refuses_images_that_are_unreadable_oversize_or_too_many() {
        let not_an_image = base64::engine::general_purpose::STANDARD.encode(b"plain text");
        assert_eq!(
            validate_images(&[ChatImageDto { data: not_an_image }]),
            Err(ChatReplyError::InvalidRequest(
                "images must be PNG, JPEG or WebP"
            ))
        );
        assert_eq!(
            validate_images(&[ChatImageDto { data: png(1025, 8) }]),
            Err(ChatReplyError::InvalidRequest(
                "an image is larger than 1024 pixels"
            ))
        );
        assert_eq!(
            validate_images(&[ChatImageDto { data: "!!".into() }]),
            Err(ChatReplyError::InvalidRequest(
                "an image is not valid base64"
            ))
        );
        let five = vec![ChatImageDto { data: png(1, 1) }; 5];
        assert_eq!(
            validate_images(&five),
            Err(ChatReplyError::InvalidRequest("too many images"))
        );
        assert_eq!(
            validate_images(&[ChatImageDto { data: png(1024, 1) }])
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn a_failed_reply_is_retried_without_saving_the_message_twice() {
        let (_directory, conversations) = store();
        let workspace = Uuid::new_v4();
        let id = conversations.create(workspace).unwrap().id;
        let service = ChatService::default();
        let failing = fake(Outcome::Transport);
        run(
            &service,
            &failing,
            &conversations,
            request(workspace, id, "Why?", Some("ctx")),
        )
        .await;

        let working = fake(Outcome::Text("Because.".into()));
        let events = run(
            &service,
            &working,
            &conversations,
            ChatReplyRequestDto {
                retry: true,
                ..request(workspace, id, "", None)
            },
        )
        .await;

        assert_eq!(
            events.last(),
            Some(&ChatReplyEventDto::Done {
                text: "Because.".into()
            })
        );
        let messages = conversations.get(workspace, id).unwrap().messages;
        assert_eq!(
            messages
                .iter()
                .map(|message| message.role)
                .collect::<Vec<_>>(),
            vec![ChatRole::User, ChatRole::Assistant]
        );
        assert_eq!(messages[0].content, "Why?");
        assert_eq!(messages[0].context.as_deref(), Some("ctx"));
        assert!(
            working.seen.lock().unwrap()[0]
                .1
                .user_prompt
                .contains("Why?")
        );
    }

    #[tokio::test]
    async fn retry_needs_a_last_message_from_the_person() {
        let (_directory, conversations) = store();
        let workspace = Uuid::new_v4();
        let id = conversations.create(workspace).unwrap().id;
        let generator = fake(Outcome::Text("unused".into()));
        let events = run(
            &ChatService::default(),
            &generator,
            &conversations,
            ChatReplyRequestDto {
                retry: true,
                ..request(workspace, id, "", None)
            },
        )
        .await;
        assert_eq!(events, vec![failed(ChatFailureDto::InvalidRequest)]);
        assert!(generator.seen.lock().unwrap().is_empty());
    }
}
