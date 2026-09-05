//! Thin HTTP adapter for grounded Ask.

use axum::Json;
use axum::extract::{Extension, State};
use fm_transport_dto::{
    ApplicationErrorDto, DeleteRagConversationRequestDto, GenerateRagAnswerRequestDto,
    GenerateRagAnswerResponseDto, ListSavedRagConversationsRequestDto, PreviewRagRequestDto,
    RagPreviewDto, ResolveRagCitationRequestDto, ResolvedRagCitationDto,
    SaveRagConversationRequestDto, SavedRagConversationDto,
};
use tower_http::request_id::RequestId;

use crate::error::{ApiError, extract_request_id};
use crate::state::AppState;

#[utoipa::path(
    post,
    path = "/api/v1/semantic/ask/preview",
    operation_id = "previewRag",
    request_body = PreviewRagRequestDto,
    responses(
        (status = 200, body = RagPreviewDto),
        (status = 400, body = ApplicationErrorDto),
        (status = 403, body = ApplicationErrorDto),
        (status = 404, body = ApplicationErrorDto),
        (status = 503, body = ApplicationErrorDto)
    )
)]
pub(crate) async fn preview_rag(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<PreviewRagRequestDto>,
) -> Result<Json<RagPreviewDto>, ApiError> {
    state
        .service
        .preview_rag(state.semantic_access(), request)
        .await
        .map(Json)
        .map_err(|error| ApiError::new(error, extract_request_id(&request_id)))
}

#[utoipa::path(
    post,
    path = "/api/v1/semantic/ask/generate",
    operation_id = "generateRagAnswer",
    request_body = GenerateRagAnswerRequestDto,
    responses(
        (status = 200, body = GenerateRagAnswerResponseDto),
        (status = 400, body = ApplicationErrorDto),
        (status = 403, body = ApplicationErrorDto),
        (status = 404, body = ApplicationErrorDto),
        (status = 503, body = ApplicationErrorDto)
    )
)]
pub(crate) async fn generate_rag_answer(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<GenerateRagAnswerRequestDto>,
) -> Result<Json<GenerateRagAnswerResponseDto>, ApiError> {
    state
        .service
        .generate_rag_answer(state.semantic_access(), request)
        .await
        .map(Json)
        .map_err(|error| ApiError::new(error, extract_request_id(&request_id)))
}

#[utoipa::path(
    post,
    path = "/api/v1/semantic/ask/conversations/save",
    operation_id = "saveRagConversation",
    request_body = SaveRagConversationRequestDto,
    responses(
        (status = 200, body = SavedRagConversationDto),
        (status = 403, body = ApplicationErrorDto),
        (status = 404, body = ApplicationErrorDto)
    )
)]
pub(crate) async fn save_rag_conversation(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<SaveRagConversationRequestDto>,
) -> Result<Json<SavedRagConversationDto>, ApiError> {
    state
        .service
        .save_rag_conversation(state.semantic_access(), request)
        .map(Json)
        .map_err(|error| ApiError::new(error, extract_request_id(&request_id)))
}

#[utoipa::path(
    post,
    path = "/api/v1/semantic/ask/conversations",
    operation_id = "listSavedRagConversations",
    request_body = ListSavedRagConversationsRequestDto,
    responses(
        (status = 200, body = Vec<SavedRagConversationDto>),
        (status = 403, body = ApplicationErrorDto)
    )
)]
pub(crate) async fn list_saved_rag_conversations(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<ListSavedRagConversationsRequestDto>,
) -> Result<Json<Vec<SavedRagConversationDto>>, ApiError> {
    state
        .service
        .list_saved_rag_conversations(state.semantic_access(), request.workspace_id)
        .map(Json)
        .map_err(|error| ApiError::new(error, extract_request_id(&request_id)))
}

#[utoipa::path(
    post,
    path = "/api/v1/semantic/ask/conversations/delete",
    operation_id = "deleteRagConversation",
    request_body = DeleteRagConversationRequestDto,
    responses(
        (status = 204),
        (status = 403, body = ApplicationErrorDto),
        (status = 404, body = ApplicationErrorDto)
    )
)]
pub(crate) async fn delete_rag_conversation(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<DeleteRagConversationRequestDto>,
) -> Result<(), ApiError> {
    state
        .service
        .delete_rag_conversation(state.semantic_access(), request)
        .map_err(|error| ApiError::new(error, extract_request_id(&request_id)))
}

#[utoipa::path(
    post,
    path = "/api/v1/semantic/ask/citations/resolve",
    operation_id = "resolveRagCitation",
    request_body = ResolveRagCitationRequestDto,
    responses(
        (status = 200, body = ResolvedRagCitationDto),
        (status = 403, body = ApplicationErrorDto),
        (status = 404, body = ApplicationErrorDto)
    )
)]
pub(crate) async fn resolve_rag_citation(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<ResolveRagCitationRequestDto>,
) -> Result<Json<ResolvedRagCitationDto>, ApiError> {
    state
        .service
        .resolve_rag_citation(state.semantic_access(), request)
        .await
        .map(Json)
        .map_err(|error| ApiError::new(error, extract_request_id(&request_id)))
}
