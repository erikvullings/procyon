//! Thin REST adapter for the file assistant (task 0321).

use axum::{
    Json,
    extract::{Extension, State},
    http::StatusCode,
};
use fm_transport_dto::{
    ApplicationErrorDto, CancelFileAssistantRequestDto, FileAssistantReplyDto,
    FileAssistantRequestDto,
};
use tower_http::request_id::RequestId;

use crate::{
    error::{ApiError, extract_request_id},
    state::AppState,
};

/// Drafts the next reply of a file-assistant conversation through a saved LLM profile.
#[utoipa::path(
    post,
    path = "/api/v1/file-assistant/reply",
    operation_id = "draftFileAssistantReply",
    request_body = FileAssistantRequestDto,
    responses(
        (status = 200, body = FileAssistantReplyDto),
        (status = 400, body = ApplicationErrorDto),
        (status = 500, body = ApplicationErrorDto),
        (status = 503, body = ApplicationErrorDto)
    )
)]
pub(crate) async fn draft_file_assistant_reply(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<FileAssistantRequestDto>,
) -> Result<Json<FileAssistantReplyDto>, ApiError> {
    state
        .service
        .draft_file_assistant_reply(request)
        .await
        .map(Json)
        .map_err(|error| ApiError::new(error, extract_request_id(&request_id)))
}

/// Cancels one running file-assistant reply.
#[utoipa::path(
    post,
    path = "/api/v1/file-assistant/reply/cancel",
    operation_id = "cancelFileAssistantReply",
    request_body = CancelFileAssistantRequestDto,
    responses((status = 204))
)]
pub(crate) async fn cancel_file_assistant_reply(
    State(state): State<AppState>,
    Json(request): Json<CancelFileAssistantRequestDto>,
) -> StatusCode {
    state.service.cancel_file_assistant_reply(request);
    StatusCode::NO_CONTENT
}
