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

use fm_agent::{Message, render_text};

use crate::agent_host::ModelSideCalls;
use crate::chat_agent::{AgentEnd, AgentReply, AgentRun, ChatTooling};
use crate::chat_conversations::{
    ChatCompaction, ChatConversation, ChatConversationError, ChatConversationService, ChatMessage,
    ChatRole,
};
use crate::file_assistant::{VisibleText, truncate_to_bytes};
use crate::llm_profiles::{
    LlmChatGeneration, LlmImageAttachment, LlmProfileError, LlmProfileService, LlmRole, ReplyFormat,
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
    /// The model did not answer in time.
    #[error("the chat model did not answer in time")]
    Timeout,
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
            ChatReplyError::Timeout => Self::Timeout,
        }
    }
}

impl From<ChatFailureDto> for ChatReplyError {
    fn from(failure: ChatFailureDto) -> Self {
        match failure {
            ChatFailureDto::InvalidRequest => Self::InvalidRequest("the request is not valid"),
            ChatFailureDto::NotFound => Self::NotFound,
            ChatFailureDto::RoleNotAssigned => Self::RoleNotAssigned,
            ChatFailureDto::ConsentRequired => Self::ConsentRequired,
            ChatFailureDto::Unavailable => Self::Unavailable,
            ChatFailureDto::Empty => Self::Empty,
            ChatFailureDto::Cancelled => Self::Cancelled,
            ChatFailureDto::ImagesNotSupported => Self::ImagesNotSupported,
            ChatFailureDto::Timeout => Self::Timeout,
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

    /// Whether the role's endpoint calls tools natively (task 0339); otherwise tools use the text
    /// protocol.
    async fn native_tool_calling(&self, _role: LlmRole) -> bool {
        false
    }

    /// The role's model context window in tokens, when known: how much conversation may be sent.
    async fn context_window(&self, _role: LlmRole) -> Option<u32> {
        None
    }

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

    async fn native_tool_calling(&self, role: LlmRole) -> bool {
        self.role_uses_native_tools(role).await
    }

    async fn context_window(&self, role: LlmRole) -> Option<u32> {
        self.role_context_window(role)
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
        tooling: Option<&ChatTooling<'_>>,
        request: ChatReplyRequestDto,
        on_event: &mut (dyn FnMut(ChatReplyEventDto) + Send),
    ) {
        let outcome = self
            .reply(generator, conversations, tooling, request, on_event)
            .await;
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
        tooling: Option<&ChatTooling<'_>>,
        request: ChatReplyRequestDto,
        on_event: &mut (dyn FnMut(ChatReplyEventDto) + Send),
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
                    steps: Vec::new(),
                    exchanges: Vec::new(),
                },
            )?
        };
        let cancellation = CancellationToken::new();
        self.lock().insert(request.request_id, cancellation.clone());
        let saved = compact_if_needed(
            generator,
            conversations,
            (request.workspace_id, request.conversation_id),
            saved,
            &cancellation,
            on_event,
        )
        .await;
        let prompts = build_prompts(&saved);
        let mut steps = Vec::new();
        let mut exchanges = Vec::new();
        let outcome = match (tooling, request.tools_folder.as_deref()) {
            (Some(tooling), Some(folder)) => {
                let question = saved
                    .messages
                    .last()
                    .map(|message| message.content.as_str())
                    .unwrap_or_default();
                let run = AgentRun {
                    system_prompt: SYSTEM_PROMPT,
                    folder,
                    question,
                    transcript: prompts,
                    images,
                    cancellation: &cancellation,
                };
                match crate::chat_agent::run(generator, tooling, run, on_event).await {
                    Ok(AgentReply {
                        end,
                        steps: looked_up,
                        exchanges: seen,
                    }) => {
                        steps = looked_up;
                        exchanges = seen;
                        match end {
                            AgentEnd::Answer(text) => Ok(text),
                            AgentEnd::Proposal(proposal) => {
                                let title = proposal.draft.title.clone();
                                on_event(ChatReplyEventDto::NoteProposal { proposal });
                                Ok(format!("I drafted a note for you to review: {title}"))
                            }
                        }
                    }
                    Err(failure) => Err(ChatReplyError::from(failure)),
                }
            }
            _ => {
                let mut forward = |fragment: &str| {
                    on_event(ChatReplyEventDto::Delta {
                        text: fragment.to_owned(),
                    });
                };
                let mut visible = VisibleText::new(&mut forward);
                generator
                    .generate_for_role(
                        LlmRole::Chat,
                        LlmChatGeneration {
                            followups: Vec::new(),
                            tools: Vec::new(),
                            system_prompt: SYSTEM_PROMPT.to_owned(),
                            user_prompt: prompts,
                            images,
                            maximum_tokens: None,
                            context_window: None,
                            temperature: None,
                            reply: ReplyFormat::Text,
                        },
                        &cancellation,
                        &mut |fragment: &str| visible.push(fragment),
                    )
                    .await
                    .map_err(|error| match error {
                        LlmProfileError::Cancelled => ChatReplyError::Cancelled,
                        LlmProfileError::ConsentRequired(_) => ChatReplyError::ConsentRequired,
                        LlmProfileError::RoleNotAssigned => ChatReplyError::RoleNotAssigned,
                        _ => ChatReplyError::Unavailable,
                    })
            }
        };
        self.lock().remove(&request.request_id);
        let text = outcome?;
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
                steps,
                exchanges,
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

/// Summarises the oldest messages before the model is asked when the conversation takes more
/// than [`fm_agent::COMPACT_AT_PERCENT`] of [`MAX_PROMPT_BYTES`] (task 0359), and stores the
/// summary in the conversation so a later reply does not summarise again. Returns the conversation
/// as it is to be sent. A failed summary is reported as a step; the oldest messages then drop out
/// in [`build_prompts`], as before.
async fn compact_if_needed(
    generator: &impl ChatGenerator,
    conversations: &ChatConversationService,
    (workspace, id): (Uuid, Uuid),
    conversation: ChatConversation,
    cancellation: &CancellationToken,
    on_event: &mut (dyn FnMut(ChatReplyEventDto) + Send),
) -> ChatConversation {
    let (messages, origins) = history_messages(&conversation);
    if !fm_agent::needs_compaction(&messages, MAX_PROMPT_BYTES) {
        return conversation;
    }
    on_event(ChatReplyEventDto::Progress {
        text: "Summarising the earlier conversation".to_owned(),
    });
    let side_calls = ModelSideCalls::new(generator, cancellation.clone()).await;
    let outcome = fm_agent::compact(&messages, MAX_PROMPT_BYTES, &side_calls, &|_| {}).await;
    let step = match outcome {
        Ok(Some(compaction)) => {
            // The cut is at one of the person's messages; it maps to that saved message.
            let Some(&covered) = origins.get(compaction.covered) else {
                return conversation;
            };
            let stored = ChatCompaction {
                summary: compaction.summary,
                covered,
            };
            let expected = conversation.messages.len();
            if conversations
                .set_compaction(workspace, id, stored, expected)
                .is_err()
            {
                return conversation;
            }
            let summarised = covered.saturating_sub(
                conversation
                    .compaction
                    .as_ref()
                    .map_or(0, |earlier| earlier.covered),
            );
            let Ok(updated) = conversations.get(workspace, id) else {
                return conversation;
            };
            on_event(ChatReplyEventDto::Step {
                text: format!("Summarised {summarised} earlier messages"),
            });
            return updated;
        }
        Ok(None) => return conversation,
        Err(_) if cancellation.is_cancelled() => return conversation,
        Err(_) => "Could not summarise the earlier conversation; its oldest part is left out",
    };
    on_event(ChatReplyEventDto::Step {
        text: step.to_owned(),
    });
    conversation
}

/// The conversation as agent messages, from its summary on, and for each message the index of
/// the saved message it came from (the summary maps to the first message after it). Used to decide
/// on and make a summary; [`build_prompts`] renders the same content.
fn history_messages(conversation: &ChatConversation) -> (Vec<Message>, Vec<usize>) {
    let start = covered(conversation);
    let mut messages = Vec::new();
    let mut origins = Vec::new();
    if let Some(compaction) = &conversation.compaction {
        messages.push(summary_message(compaction));
        origins.push(start);
    }
    for (index, message) in conversation.messages.iter().enumerate().skip(start) {
        for part in message_parts(message) {
            messages.push(part);
            origins.push(index);
        }
    }
    (messages, origins)
}

/// How many leading messages the summary stands for.
fn covered(conversation: &ChatConversation) -> usize {
    conversation
        .compaction
        .as_ref()
        .map_or(0, |compaction| compaction.covered)
        .min(conversation.messages.len())
}

fn summary_message(compaction: &ChatCompaction) -> Message {
    fm_agent::Compaction {
        summary: compaction.summary.clone(),
        covered: compaction.covered,
    }
    .message()
}

/// One saved message as agent messages: the person's message with its context, or a reply
/// preceded by the lookups the model made for it.
fn message_parts(message: &ChatMessage) -> Vec<Message> {
    match message.role {
        ChatRole::User => {
            let context = message
                .context
                .as_deref()
                .map(|text| format!("Context attached by the person:\n{text}\n\n"))
                .unwrap_or_default();
            vec![Message::User(format!(
                "{context}Person: {}",
                message.content
            ))]
        }
        ChatRole::Assistant => {
            let mut parts: Vec<Message> = message.exchanges.iter().map(past_exchange).collect();
            parts.push(Message::Assistant {
                text: message.content.clone(),
                calls: Vec::new(),
            });
            parts
        }
    }
}

/// An earlier reply's lookup, worded so it reads as history rather than as a call to repeat:
/// the conversation goes to the model as text, and `TOOL` lines in it could be taken for the
/// format of a new call.
fn past_exchange(message: &Message) -> Message {
    match message {
        Message::Assistant { text, calls } if !calls.is_empty() => {
            let mut said = Message::prose(text);
            for call in calls {
                if !said.is_empty() {
                    said.push('\n');
                }
                said.push_str(&format!(
                    "(looked up with {} {})",
                    call.name, call.arguments
                ));
            }
            Message::Assistant {
                text: said,
                calls: Vec::new(),
            }
        }
        other => other.clone(),
    }
}

/// Renders the conversation for the model: the summary of the oldest messages, then the newest
/// messages that fit in the budget. The latest message is always included.
fn build_prompts(conversation: &ChatConversation) -> String {
    let summary = conversation
        .compaction
        .as_ref()
        .map(|compaction| render_text(&[summary_message(compaction)]))
        .unwrap_or_default();
    let budget = MAX_PROMPT_BYTES.saturating_sub(summary.len());
    let mut sections: Vec<String> = Vec::new();
    let mut total = 0;
    for message in conversation.messages[covered(conversation)..].iter().rev() {
        let section = render_text(&message_parts(message));
        if total + section.len() > budget && !sections.is_empty() {
            break;
        }
        total += section.len();
        sections.push(section);
    }
    sections.reverse();
    format!("{summary}{}", sections.concat())
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
            tools_folder: None,
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
            .stream(generator, conversations, None, request, &mut |event| {
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
                .stream(&generator, &conversations, None, req, &mut |event| {
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
                steps: Vec::new(),
                exchanges: Vec::new(),
            });
        }
        let conversation = ChatConversation {
            id: Uuid::new_v4(),
            title: String::new(),
            updated_at: String::new(),
            messages,
            compaction: None,
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

    /// Hands out scripted answers, one per call, and records every request.
    struct Scripted {
        answers: StdMutex<std::collections::VecDeque<&'static str>>,
        seen: StdMutex<Vec<LlmChatGeneration>>,
    }

    #[async_trait]
    impl ChatGenerator for Scripted {
        async fn accepts_images(&self, _role: LlmRole) -> Result<bool, LlmProfileError> {
            Ok(false)
        }

        async fn generate_for_role(
            &self,
            _role: LlmRole,
            generation: LlmChatGeneration,
            _cancellation: &CancellationToken,
            on_delta: &mut (dyn for<'a> FnMut(&'a str) + Send),
        ) -> Result<String, LlmProfileError> {
            self.seen.lock().unwrap().push(generation);
            let text = self
                .answers
                .lock()
                .unwrap()
                .pop_front()
                .ok_or(LlmProfileError::Transport)?;
            on_delta(text);
            Ok(text.to_owned())
        }
    }

    fn scripted(answers: &[&'static str]) -> Scripted {
        Scripted {
            answers: StdMutex::new(answers.iter().copied().collect()),
            seen: StdMutex::new(Vec::new()),
        }
    }

    fn skill_index() -> &'static std::sync::Arc<fm_agent::SkillIndex> {
        static INDEX: std::sync::OnceLock<std::sync::Arc<fm_agent::SkillIndex>> =
            std::sync::OnceLock::new();
        INDEX.get_or_init(|| std::sync::Arc::new(fm_agent::SkillIndex::new()))
    }

    fn local_tooling(providers: &fm_vfs::ProviderRegistry) -> ChatTooling<'_> {
        ChatTooling {
            providers,
            notes: None,
            skill_scope: crate::agent_host::SkillScope::WorkingFolder,
            skills: skill_index(),
        }
    }

    fn folder_request(
        workspace: Uuid,
        conversation: Uuid,
        folder: &std::path::Path,
    ) -> ChatReplyRequestDto {
        let mut request = request(workspace, conversation, "What is in a.txt?", None);
        request.tools_folder = Some(url::Url::from_directory_path(folder).unwrap().to_string());
        request
    }

    #[tokio::test]
    async fn with_a_folder_the_model_reads_a_file_and_the_steps_are_reported() {
        let (_directory, conversations) = store();
        let workspace = Uuid::new_v4();
        let id = conversations.create(workspace).unwrap().id;
        let folder = tempdir().unwrap();
        std::fs::write(folder.path().join("a.txt"), "alpha beta").unwrap();
        let mut providers = fm_vfs::ProviderRegistry::new();
        providers.register(std::sync::Arc::new(
            fm_vfs_local::LocalFileSystemProvider::new(),
        ));
        let generator = scripted(&[
            r#"TOOL {"tool":"read_file","path":"a.txt"}"#,
            "It says alpha beta.",
        ]);
        let mut seen = Vec::new();
        ChatService::default()
            .stream(
                &generator,
                &conversations,
                Some(&local_tooling(&providers)),
                folder_request(workspace, id, folder.path()),
                &mut |event| seen.push(event),
            )
            .await;

        assert!(seen.contains(&ChatReplyEventDto::Step {
            text: "Read a.txt".into()
        }));
        assert_eq!(
            seen.last(),
            Some(&ChatReplyEventDto::Done {
                text: "It says alpha beta.".into()
            })
        );
        let requests = generator.seen.lock().unwrap();
        assert!(requests[0].system_prompt.contains("read_file"));
        assert!(requests[1].user_prompt.contains("alpha beta"));
        let saved = conversations.get(workspace, id).unwrap();
        assert_eq!(saved.messages.len(), 2);
        assert_eq!(saved.messages[1].content, "It says alpha beta.");
        // The lookups stay with the reply, so they are still shown when the conversation reopens.
        assert_eq!(saved.messages[1].steps, vec!["Read a.txt".to_owned()]);
    }

    #[tokio::test]
    async fn a_later_question_still_sees_what_the_tools_found_earlier() {
        let (_directory, conversations) = store();
        let workspace = Uuid::new_v4();
        let id = conversations.create(workspace).unwrap().id;
        let folder = tempdir().unwrap();
        std::fs::write(folder.path().join("a.txt"), "alpha beta").unwrap();
        let mut providers = fm_vfs::ProviderRegistry::new();
        providers.register(std::sync::Arc::new(
            fm_vfs_local::LocalFileSystemProvider::new(),
        ));
        let generator = scripted(&[
            r#"TOOL {"tool":"read_file","path":"a.txt"}"#,
            "It says alpha beta.",
            "Yes, beta.",
        ]);
        let tooling = local_tooling(&providers);
        let service = ChatService::default();
        service
            .stream(
                &generator,
                &conversations,
                Some(&tooling),
                folder_request(workspace, id, folder.path()),
                &mut |_| {},
            )
            .await;
        let saved = conversations.get(workspace, id).unwrap();
        // The call and its result are saved with the reply, the answer itself is not repeated.
        assert!(matches!(
            saved.messages[1].exchanges.as_slice(),
            [Message::Assistant { calls, .. }, Message::ToolResult { text, .. }]
                if calls[0].name == "read_file" && text.contains("alpha beta")
        ));

        let mut next = folder_request(workspace, id, folder.path());
        next.content = "Does it mention beta?".into();
        service
            .stream(
                &generator,
                &conversations,
                Some(&tooling),
                next,
                &mut |_| {},
            )
            .await;
        let requests = generator.seen.lock().unwrap();
        let prompt = &requests[2].user_prompt;
        assert!(prompt.contains("alpha beta"), "{prompt}");
        assert!(prompt.contains("(looked up with read_file"), "{prompt}");
        // Earlier calls read as history, never as a call line to copy.
        assert!(!prompt.contains("TOOL {"), "{prompt}");
    }

    #[tokio::test]
    async fn a_long_conversation_is_summarised_once_and_the_summary_is_kept() {
        let (_directory, conversations) = store();
        let workspace = Uuid::new_v4();
        let id = conversations.create(workspace).unwrap().id;
        for index in 0..12 {
            for role in [ChatRole::User, ChatRole::Assistant] {
                conversations
                    .append(
                        workspace,
                        id,
                        ChatMessage {
                            role,
                            content: format!("{index}:{}", "w".repeat(4 * 1024)),
                            context: None,
                            steps: Vec::new(),
                            exchanges: Vec::new(),
                        },
                    )
                    .unwrap();
            }
        }
        let service = ChatService::default();
        let generator = fake(Outcome::Text("They compared twelve files.".into()));
        let events = run(
            &service,
            &generator,
            &conversations,
            request(workspace, id, "And the last one?", None),
        )
        .await;
        assert!(
            events
                .iter()
                .any(|event| matches!(event, ChatReplyEventDto::Step { text } if text.starts_with("Summarised")))
        );
        let saved = conversations.get(workspace, id).unwrap();
        let compaction = saved.compaction.clone().unwrap();
        assert!(compaction.covered > 0 && compaction.covered < 25);
        // The summary ends before one of the person's messages.
        assert_eq!(saved.messages[compaction.covered].role, ChatRole::User);
        {
            let requests = generator.seen.lock().unwrap();
            // Summary calls (a long history is summarised in parts), then the reply.
            assert!(requests.len() >= 2);
            let prompt = &requests.last().unwrap().1.user_prompt;
            assert!(prompt.starts_with("Summary of the conversation so far:\nThey compared"));
            assert!(!prompt.contains("Person: 0:"));
            assert!(prompt.ends_with("Person: And the last one?\n\n"));
        }

        // Reopened, the stored summary is used as it is: no new summary call.
        let reopened =
            ChatConversationService::new(&SettingsStore::new(_directory.path())).unwrap();
        assert_eq!(
            reopened.get(workspace, id).unwrap().compaction,
            Some(compaction)
        );
        let generator = fake(Outcome::Text("Fine.".into()));
        run(
            &service,
            &generator,
            &reopened,
            request(workspace, id, "Thanks", None),
        )
        .await;
        assert_eq!(generator.seen.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn the_model_cannot_leave_the_folder() {
        let (_directory, conversations) = store();
        let workspace = Uuid::new_v4();
        let id = conversations.create(workspace).unwrap().id;
        let outer = tempdir().unwrap();
        std::fs::write(outer.path().join("secret.txt"), "top secret").unwrap();
        let folder = outer.path().join("inner");
        std::fs::create_dir(&folder).unwrap();
        let mut providers = fm_vfs::ProviderRegistry::new();
        providers.register(std::sync::Arc::new(
            fm_vfs_local::LocalFileSystemProvider::new(),
        ));
        let generator = scripted(&[r#"TOOL {"tool":"read_file","path":"../secret.txt"}"#, "No."]);
        ChatService::default()
            .stream(
                &generator,
                &conversations,
                Some(&local_tooling(&providers)),
                folder_request(workspace, id, &folder),
                &mut |_| {},
            )
            .await;

        assert!(
            !generator.seen.lock().unwrap()[1]
                .user_prompt
                .contains("top secret")
        );
    }

    #[tokio::test]
    async fn without_a_folder_no_tools_are_offered() {
        let (_directory, conversations) = store();
        let workspace = Uuid::new_v4();
        let id = conversations.create(workspace).unwrap().id;
        let generator = fake(Outcome::Text("Hi".into()));
        run(
            &ChatService::default(),
            &generator,
            &conversations,
            request(workspace, id, "Hello", None),
        )
        .await;

        let seen = generator.seen.lock().unwrap();
        assert!(!seen[0].1.system_prompt.contains("TOOL"));
        assert!(seen[0].1.tools.is_empty());
    }

    fn local_providers() -> fm_vfs::ProviderRegistry {
        let mut providers = fm_vfs::ProviderRegistry::new();
        providers.register(std::sync::Arc::new(
            fm_vfs_local::LocalFileSystemProvider::new(),
        ));
        providers
    }

    /// Notes that offer one note as a skill and draft whatever the model proposes.
    struct ChatNotes;

    #[async_trait]
    impl fm_agent::NotesAccess for ChatNotes {
        async fn run(
            &self,
            call: fm_agent::NoteToolCall,
        ) -> fm_agent::ToolOutcome<fm_agent::NoteProposal> {
            match call {
                fm_agent::NoteToolCall::Read { path } => fm_agent::ToolOutcome::output(
                    format!("Read note {path}"),
                    format!("Note {path}:\n1. certbot renew"),
                ),
                fm_agent::NoteToolCall::Search { .. } => {
                    fm_agent::ToolOutcome::output("Searched notes", "1. ops/nginx.md")
                }
                fm_agent::NoteToolCall::Propose { path, content, .. } => {
                    fm_agent::ToolOutcome::Interrupt(fm_agent::NoteProposal {
                        path,
                        title: "Renew certificates".into(),
                        file_name: "renew-certificates.md".into(),
                        tags: Vec::new(),
                        content,
                        redactions: 0,
                    })
                }
            }
        }

        async fn skills(&self, _query: &str) -> Vec<fm_agent::NoteSkill> {
            vec![fm_agent::NoteSkill {
                name: "nginx-cert".into(),
                description: "Use when replacing a certificate".into(),
                path: "ops/nginx.md".into(),
            }]
        }
    }

    async fn stream_with(
        generator: &impl ChatGenerator,
        tooling: &ChatTooling<'_>,
        folder: &std::path::Path,
    ) -> (
        Vec<ChatReplyEventDto>,
        ChatConversationService,
        Uuid,
        Uuid,
        tempfile::TempDir,
    ) {
        let (directory, conversations) = store();
        let workspace = Uuid::new_v4();
        let id = conversations.create(workspace).unwrap().id;
        let mut seen = Vec::new();
        ChatService::default()
            .stream(
                generator,
                &conversations,
                Some(tooling),
                folder_request(workspace, id, folder),
                &mut |event| seen.push(event),
            )
            .await;
        (seen, conversations, workspace, id, directory)
    }

    #[tokio::test]
    async fn the_model_lists_and_searches_the_folder_in_one_reply() {
        let folder = tempdir().unwrap();
        std::fs::write(folder.path().join("a.txt"), "alpha beta").unwrap();
        std::fs::write(folder.path().join("b.txt"), "gamma").unwrap();
        let providers = local_providers();
        let generator = scripted(&[
            "TOOL {\"tool\":\"list_dir\",\"path\":\".\"}\nTOOL {\"tool\":\"grep\",\"pattern\":\"beta\"}",
            "a.txt mentions beta.",
        ]);
        let (seen, ..) = stream_with(&generator, &local_tooling(&providers), folder.path()).await;

        let steps: Vec<&str> = seen
            .iter()
            .filter_map(|event| match event {
                ChatReplyEventDto::Step { text } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(steps.len(), 2, "{seen:?}");
        let second = &generator.seen.lock().unwrap()[1].user_prompt;
        assert!(second.contains("b.txt"), "the listing reaches the model");
        assert!(
            second.contains("alpha beta"),
            "the search hit reaches the model: {second}"
        );
        assert_eq!(
            seen.last(),
            Some(&ChatReplyEventDto::Done {
                text: "a.txt mentions beta.".into()
            })
        );
    }

    #[tokio::test]
    async fn notes_and_project_skills_are_offered_and_load_in_the_chat() {
        let folder = tempdir().unwrap();
        let skill = folder.path().join(".claude/skills/release");
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(
            skill.join("SKILL.md"),
            "---\nname: release\ndescription: Use when cutting a release\n---\nTag, then push.\n",
        )
        .unwrap();
        let providers = local_providers();
        let tooling = ChatTooling {
            notes: Some(&ChatNotes),
            ..local_tooling(&providers)
        };
        let generator = scripted(&[
            "TOOL {\"tool\":\"read_skill\",\"name\":\"nginx-cert\"}\nTOOL {\"tool\":\"read_skill\",\"name\":\"release\"}",
            "Renew with certbot, then tag.",
        ]);
        let (seen, ..) = stream_with(&generator, &tooling, folder.path()).await;

        let requests = generator.seen.lock().unwrap();
        let system = &requests[0].system_prompt;
        assert!(system.contains("- nginx-cert: Use when replacing a certificate [note]"));
        assert!(system.contains("- release: Use when cutting a release [project]"));
        assert!(requests[1].user_prompt.contains("1. certbot renew"));
        assert!(requests[1].user_prompt.contains("Tag, then push."));
        assert!(matches!(seen.last(), Some(ChatReplyEventDto::Done { .. })));
    }

    #[tokio::test]
    async fn a_proposed_note_goes_to_review_and_the_reply_says_so() {
        let folder = tempdir().unwrap();
        let providers = local_providers();
        let tooling = ChatTooling {
            notes: Some(&ChatNotes),
            ..local_tooling(&providers)
        };
        let generator = scripted(&[
            r#"TOOL {"tool":"propose_note","title":"Renew","content":"certbot renew"}"#,
        ]);
        let (seen, conversations, workspace, id, _directory) =
            stream_with(&generator, &tooling, folder.path()).await;

        let proposal = seen.iter().find_map(|event| match event {
            ChatReplyEventDto::NoteProposal { proposal } => Some(proposal),
            _ => None,
        });
        let proposal = proposal.expect("a proposal event");
        assert_eq!(proposal.path, None);
        assert_eq!(proposal.draft.content, "certbot renew");
        let saved = conversations.get(workspace, id).unwrap();
        assert!(saved.messages[1].content.contains("Renew certificates"));
    }

    /// A model whose endpoint calls tools natively.
    struct Native(Scripted);

    #[async_trait]
    impl ChatGenerator for Native {
        async fn accepts_images(&self, role: LlmRole) -> Result<bool, LlmProfileError> {
            self.0.accepts_images(role).await
        }

        async fn native_tool_calling(&self, _role: LlmRole) -> bool {
            true
        }

        async fn generate_for_role(
            &self,
            role: LlmRole,
            generation: LlmChatGeneration,
            cancellation: &CancellationToken,
            on_delta: &mut (dyn for<'a> FnMut(&'a str) + Send),
        ) -> Result<String, LlmProfileError> {
            self.0
                .generate_for_role(role, generation, cancellation, on_delta)
                .await
        }
    }

    #[tokio::test]
    async fn the_chat_follows_the_folders_agents_and_claude_files() {
        let folder = tempdir().unwrap();
        std::fs::write(folder.path().join("AGENTS.md"), "Answer in haiku.").unwrap();
        std::fs::write(folder.path().join("CLAUDE.md"), "Prefer pnpm.").unwrap();
        let providers = local_providers();
        let generator = scripted(&["Done."]);
        stream_with(&generator, &local_tooling(&providers), folder.path()).await;
        let requests = generator.seen.lock().unwrap();
        let prompt = &requests[0].system_prompt;
        assert!(
            prompt.contains("### AGENTS.md\nAnswer in haiku."),
            "{prompt}"
        );
        assert!(prompt.contains("### CLAUDE.md\nPrefer pnpm."), "{prompt}");
    }

    #[tokio::test]
    async fn natively_the_tools_go_as_specs_and_a_written_back_call_runs() {
        let folder = tempdir().unwrap();
        std::fs::write(folder.path().join("a.txt"), "alpha beta").unwrap();
        let providers = local_providers();
        let generator = Native(scripted(&[
            r#"TOOL {"name":"read_file","arguments":{"path":"a.txt"}}"#,
            "It says alpha beta.",
        ]));
        let (seen, ..) = stream_with(&generator, &local_tooling(&providers), folder.path()).await;

        let requests = generator.0.seen.lock().unwrap();
        assert!(!requests[0].system_prompt.contains("TOOL {json}"));
        let names: Vec<&str> = requests[0].tools.iter().map(|tool| tool.name).collect();
        assert!(names.contains(&"read_file") && names.contains(&"read_skill"));
        assert!(
            !names.contains(&"run_command"),
            "the chat cannot run commands"
        );
        // The call and its result follow the question as structured messages (task 0341).
        assert!(!requests[1].user_prompt.contains("alpha beta"));
        let followups = &requests[1].followups;
        let fm_agent::Message::Assistant { calls, .. } = &followups[0] else {
            panic!("the reply with its call comes first: {followups:?}");
        };
        assert_eq!(calls[0].name, "read_file");
        assert!(matches!(
            &followups[1],
            fm_agent::Message::ToolResult { call_id, text, .. }
                if *call_id == calls[0].id && text.contains("alpha beta")
        ));
        assert!(matches!(seen.last(), Some(ChatReplyEventDto::Done { .. })));
    }
}
