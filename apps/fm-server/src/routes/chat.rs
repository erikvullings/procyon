//! Thin REST adapter for chat conversations and replies (task 0324).

use std::convert::Infallible;

use axum::{
    Json,
    body::{Body, Bytes},
    extract::{Extension, Path, State},
    http::{StatusCode, header},
    response::Response,
};
use fm_transport_dto::{
    CancelChatReplyRequestDto, ChatConversationDto, ChatConversationSummaryDto, ChatReplyEventDto,
    ChatReplyRequestDto, ForkChatConversationRequestDto, RenameChatConversationRequestDto,
};
use tokio::sync::mpsc;
use tower_http::request_id::RequestId;
use uuid::Uuid;

use crate::error::{ApiError, extract_request_id};
use crate::state::AppState;

/// Lists a workspace's saved chat conversations, most recently changed first.
#[utoipa::path(
    get,
    path = "/api/v1/workspaces/{workspaceId}/chat/conversations",
    operation_id = "listChatConversations",
    params(("workspaceId" = Uuid, Path, description = "The workspace that owns the conversations")),
    responses((status = 200, description = "Conversation summaries", body = Vec<ChatConversationSummaryDto>))
)]
pub(crate) async fn list_chat_conversations(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(workspace_id): Path<Uuid>,
) -> Result<Json<Vec<ChatConversationSummaryDto>>, ApiError> {
    let request_id = extract_request_id(&request_id);
    state
        .service
        .list_chat_conversations(workspace_id)
        .map(Json)
        .map_err(|error| ApiError::new(error, request_id))
}

/// Starts an empty chat conversation in a workspace.
#[utoipa::path(
    post,
    path = "/api/v1/workspaces/{workspaceId}/chat/conversations",
    operation_id = "createChatConversation",
    params(("workspaceId" = Uuid, Path, description = "The workspace that owns the conversation")),
    responses((status = 200, description = "The new conversation", body = ChatConversationDto))
)]
pub(crate) async fn create_chat_conversation(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(workspace_id): Path<Uuid>,
) -> Result<Json<ChatConversationDto>, ApiError> {
    let request_id = extract_request_id(&request_id);
    state
        .service
        .create_chat_conversation(workspace_id)
        .map(Json)
        .map_err(|error| ApiError::new(error, request_id))
}

/// Loads one chat conversation with its messages.
#[utoipa::path(
    get,
    path = "/api/v1/workspaces/{workspaceId}/chat/conversations/{conversationId}",
    operation_id = "getChatConversation",
    params(
        ("workspaceId" = Uuid, Path, description = "The workspace that owns the conversation"),
        ("conversationId" = Uuid, Path, description = "The conversation to load"),
    ),
    responses(
        (status = 200, description = "The conversation", body = ChatConversationDto),
        (status = 404, description = "No such conversation", body = fm_transport_dto::ApplicationErrorDto),
    )
)]
pub(crate) async fn get_chat_conversation(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path((workspace_id, conversation_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<ChatConversationDto>, ApiError> {
    let request_id = extract_request_id(&request_id);
    state
        .service
        .get_chat_conversation(workspace_id, conversation_id)
        .map(Json)
        .map_err(|error| ApiError::new(error, request_id))
}

/// Renames a chat conversation.
#[utoipa::path(
    patch,
    path = "/api/v1/workspaces/{workspaceId}/chat/conversations/{conversationId}",
    operation_id = "renameChatConversation",
    params(
        ("workspaceId" = Uuid, Path, description = "The workspace that owns the conversation"),
        ("conversationId" = Uuid, Path, description = "The conversation to rename"),
    ),
    request_body = RenameChatConversationRequestDto,
    responses(
        (status = 200, description = "The renamed conversation", body = ChatConversationDto),
        (status = 404, description = "No such conversation", body = fm_transport_dto::ApplicationErrorDto),
    )
)]
pub(crate) async fn rename_chat_conversation(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path((workspace_id, conversation_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<RenameChatConversationRequestDto>,
) -> Result<Json<ChatConversationDto>, ApiError> {
    let request_id = extract_request_id(&request_id);
    state
        .service
        .rename_chat_conversation(workspace_id, conversation_id, request)
        .map(Json)
        .map_err(|error| ApiError::new(error, request_id))
}

/// Starts a new chat conversation with the first messages of another ("fork from here").
#[utoipa::path(
    post,
    path = "/api/v1/workspaces/{workspaceId}/chat/conversations/{conversationId}/fork",
    operation_id = "forkChatConversation",
    params(
        ("workspaceId" = Uuid, Path, description = "The workspace that owns the conversation"),
        ("conversationId" = Uuid, Path, description = "The conversation to fork"),
    ),
    request_body = ForkChatConversationRequestDto,
    responses(
        (status = 200, description = "The new conversation", body = ChatConversationDto),
        (status = 400, description = "The fork point is past the end", body = fm_transport_dto::ApplicationErrorDto),
        (status = 404, description = "No such conversation", body = fm_transport_dto::ApplicationErrorDto),
    )
)]
pub(crate) async fn fork_chat_conversation(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path((workspace_id, conversation_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<ForkChatConversationRequestDto>,
) -> Result<Json<ChatConversationDto>, ApiError> {
    let request_id = extract_request_id(&request_id);
    state
        .service
        .fork_chat_conversation(workspace_id, conversation_id, request)
        .map(Json)
        .map_err(|error| ApiError::new(error, request_id))
}

/// Deletes a chat conversation and everything saved in it.
#[utoipa::path(
    delete,
    path = "/api/v1/workspaces/{workspaceId}/chat/conversations/{conversationId}",
    operation_id = "deleteChatConversation",
    params(
        ("workspaceId" = Uuid, Path, description = "The workspace that owns the conversation"),
        ("conversationId" = Uuid, Path, description = "The conversation to delete"),
    ),
    responses(
        (status = 204, description = "The conversation was deleted"),
        (status = 404, description = "No such conversation", body = fm_transport_dto::ApplicationErrorDto),
    )
)]
pub(crate) async fn delete_chat_conversation(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path((workspace_id, conversation_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    let request_id = extract_request_id(&request_id);
    state
        .service
        .delete_chat_conversation(workspace_id, conversation_id)
        .map_err(|error| ApiError::new(error, request_id))?;
    Ok(StatusCode::NO_CONTENT)
}

/// Streams the next reply of a chat conversation as newline-delimited JSON events: fragments as
/// they arrive, then exactly one `done` or `failed` event. Every outcome, including a refused
/// request, arrives as an event on this 200 response.
#[utoipa::path(
    post,
    path = "/api/v1/chat/reply/stream",
    operation_id = "streamChatReply",
    request_body = ChatReplyRequestDto,
    responses(
        (
            status = 200,
            description = "Newline-delimited JSON chat events.",
            content_type = "application/x-ndjson",
            body = ChatReplyEventDto
        )
    )
)]
pub(crate) async fn stream_chat_reply(
    State(state): State<AppState>,
    Json(request): Json<ChatReplyRequestDto>,
) -> Response {
    let (sender, receiver) = mpsc::unbounded_channel::<ChatReplyEventDto>();
    let service = state.service.clone();
    let request_id = request.request_id;
    let access = state.semantic_access.clone();
    let allowed = tools_folder_allowed(request.tools_folder.as_deref(), &state.accessible_roots);
    tokio::spawn(async move {
        if !allowed {
            let _ = sender.send(ChatReplyEventDto::Failed {
                reason: fm_transport_dto::ChatFailureDto::InvalidRequest,
            });
            return;
        }
        let canceller = service.clone();
        service
            .stream_chat_reply(&access, request, &mut |event| {
                // The client went away: stop the reply instead of paying for an unread one.
                if sender.send(event).is_err() {
                    canceller.cancel_chat_reply(CancelChatReplyRequestDto { request_id });
                }
            })
            .await;
    });
    let lines = futures::stream::unfold(receiver, |mut receiver| async move {
        let event = receiver.recv().await?;
        let mut line = serde_json::to_vec(&event).unwrap_or_default();
        line.push(b'\n');
        Some((Ok::<_, Infallible>(Bytes::from(line)), receiver))
    });
    Response::builder()
        .header(header::CONTENT_TYPE, "application/x-ndjson")
        .body(Body::from_stream(lines))
        .unwrap_or_default()
}

/// Whether the folder the tools may look into is inside the accessible roots. A request without
/// tools always passes; a folder that is not a local `file:` location is only checked by the
/// provider that serves it, like every other file route.
fn tools_folder_allowed(folder: Option<&str>, roots: &[std::path::PathBuf]) -> bool {
    let Some(folder) = folder else {
        return true;
    };
    match url::Url::parse(folder) {
        Ok(url) if url.scheme() == "file" => url.to_file_path().is_ok_and(|path| {
            crate::accessible_roots::validate_within_accessible_roots(&path, roots).is_ok()
        }),
        Ok(_) => true,
        Err(_) => false,
    }
}

/// Stops one running chat reply. The person's message stays saved; no reply is saved.
#[utoipa::path(
    post,
    path = "/api/v1/chat/reply/cancel",
    operation_id = "cancelChatReply",
    request_body = CancelChatReplyRequestDto,
    responses((status = 204))
)]
pub(crate) async fn cancel_chat_reply(
    State(state): State<AppState>,
    Json(request): Json<CancelChatReplyRequestDto>,
) -> StatusCode {
    state.service.cancel_chat_reply(request);
    StatusCode::NO_CONTENT
}
