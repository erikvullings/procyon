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
    ChatReplyRequestDto, RenameChatConversationRequestDto,
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
    tokio::spawn(async move {
        let canceller = service.clone();
        service
            .stream_chat_reply(request, &mut |event| {
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
