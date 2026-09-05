//! Thin HTTP adapter for representative document summaries.

use axum::Json;
use axum::extract::{Extension, State};
use fm_transport_dto::{
    ApplicationErrorDto, DocumentSummaryDto, DocumentSummaryPreviewDto,
    GenerateDocumentSummaryRequestDto, GetDocumentSummaryRequestDto,
    PreviewDocumentSummaryRequestDto,
};
use tower_http::request_id::RequestId;

use crate::error::{ApiError, extract_request_id};
use crate::state::AppState;

#[utoipa::path(
    post,
    path = "/api/v1/semantic/document-summary/preview",
    operation_id = "previewDocumentSummary",
    request_body = PreviewDocumentSummaryRequestDto,
    responses(
        (status = 200, body = DocumentSummaryPreviewDto),
        (status = 400, body = ApplicationErrorDto),
        (status = 403, body = ApplicationErrorDto),
        (status = 404, body = ApplicationErrorDto),
        (status = 503, body = ApplicationErrorDto)
    )
)]
pub(crate) async fn preview_document_summary(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<PreviewDocumentSummaryRequestDto>,
) -> Result<Json<DocumentSummaryPreviewDto>, ApiError> {
    state
        .service
        .preview_document_summary(state.semantic_access(), request)
        .await
        .map(Json)
        .map_err(|error| ApiError::new(error, extract_request_id(&request_id)))
}

#[utoipa::path(
    post,
    path = "/api/v1/semantic/document-summary/generate",
    operation_id = "generateDocumentSummary",
    request_body = GenerateDocumentSummaryRequestDto,
    responses(
        (status = 200, body = DocumentSummaryDto),
        (status = 400, body = ApplicationErrorDto),
        (status = 403, body = ApplicationErrorDto),
        (status = 404, body = ApplicationErrorDto),
        (status = 503, body = ApplicationErrorDto)
    )
)]
pub(crate) async fn generate_document_summary(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<GenerateDocumentSummaryRequestDto>,
) -> Result<Json<DocumentSummaryDto>, ApiError> {
    state
        .service
        .generate_document_summary(state.semantic_access(), request)
        .await
        .map(Json)
        .map_err(|error| ApiError::new(error, extract_request_id(&request_id)))
}

#[utoipa::path(
    post,
    path = "/api/v1/semantic/document-summary",
    operation_id = "getDocumentSummary",
    request_body = GetDocumentSummaryRequestDto,
    responses(
        (status = 200, body = Option<DocumentSummaryDto>),
        (status = 403, body = ApplicationErrorDto),
        (status = 404, body = ApplicationErrorDto),
        (status = 503, body = ApplicationErrorDto)
    )
)]
pub(crate) async fn get_document_summary(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<GetDocumentSummaryRequestDto>,
) -> Result<Json<Option<DocumentSummaryDto>>, ApiError> {
    state
        .service
        .get_document_summary(state.semantic_access(), request)
        .await
        .map(Json)
        .map_err(|error| ApiError::new(error, extract_request_id(&request_id)))
}
