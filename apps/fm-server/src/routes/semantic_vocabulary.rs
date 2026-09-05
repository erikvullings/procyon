//! Thin REST adapter for device-local SKOS vocabularies.

use axum::Json;
use axum::extract::{Extension, State};
use axum::http::StatusCode;
use fm_transport_dto::{
    ApplicationErrorDto, AttachSemanticVocabularyRequestDto, DeleteSemanticVocabularyImpactDto,
    DeleteSemanticVocabularyRequestDto, ExportSemanticVocabularyResponseDto,
    ImportSemanticVocabularyRequestDto, ReviewConceptCandidateRequestDto, SemanticVocabularyDto,
    VocabularyIdRequestDto,
};
use tower_http::request_id::RequestId;

use crate::error::{ApiError, extract_request_id};
use crate::state::AppState;

#[utoipa::path(
    get,
    path = "/api/v1/semantic/vocabularies",
    operation_id = "listSemanticVocabularies",
    responses(
        (status = 200, body = Vec<SemanticVocabularyDto>),
        (status = 403, body = ApplicationErrorDto)
    )
)]
pub(crate) async fn list(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
) -> Result<Json<Vec<SemanticVocabularyDto>>, ApiError> {
    state
        .service
        .list_semantic_vocabularies(state.semantic_access())
        .await
        .map(Json)
        .map_err(|error| ApiError::new(error, extract_request_id(&request_id)))
}

#[utoipa::path(
    post,
    path = "/api/v1/semantic/vocabularies/import",
    operation_id = "importSemanticVocabulary",
    request_body = ImportSemanticVocabularyRequestDto,
    responses(
        (status = 201, body = SemanticVocabularyDto),
        (status = 400, body = ApplicationErrorDto),
        (status = 403, body = ApplicationErrorDto)
    )
)]
pub(crate) async fn import(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<ImportSemanticVocabularyRequestDto>,
) -> Result<(StatusCode, Json<SemanticVocabularyDto>), ApiError> {
    state
        .service
        .import_semantic_vocabulary(state.semantic_access(), request)
        .await
        .map(|vocabulary| (StatusCode::CREATED, Json(vocabulary)))
        .map_err(|error| ApiError::new(error, extract_request_id(&request_id)))
}

#[utoipa::path(
    post,
    path = "/api/v1/semantic/vocabularies/export",
    operation_id = "exportSemanticVocabulary",
    request_body = VocabularyIdRequestDto,
    responses(
        (status = 200, body = ExportSemanticVocabularyResponseDto),
        (status = 400, body = ApplicationErrorDto)
    )
)]
pub(crate) async fn export(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<VocabularyIdRequestDto>,
) -> Result<Json<ExportSemanticVocabularyResponseDto>, ApiError> {
    state
        .service
        .export_semantic_vocabulary(state.semantic_access(), &request.vocabulary_id)
        .await
        .map(Json)
        .map_err(|error| ApiError::new(error, extract_request_id(&request_id)))
}

#[utoipa::path(
    post,
    path = "/api/v1/semantic/vocabularies/attach",
    operation_id = "attachSemanticVocabulary",
    request_body = AttachSemanticVocabularyRequestDto,
    responses(
        (status = 200, body = SemanticVocabularyDto),
        (status = 400, body = ApplicationErrorDto),
        (status = 403, body = ApplicationErrorDto)
    )
)]
pub(crate) async fn attach(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<AttachSemanticVocabularyRequestDto>,
) -> Result<Json<SemanticVocabularyDto>, ApiError> {
    state
        .service
        .attach_semantic_vocabulary(state.semantic_access(), request)
        .await
        .map(Json)
        .map_err(|error| ApiError::new(error, extract_request_id(&request_id)))
}

#[utoipa::path(
    post,
    path = "/api/v1/semantic/vocabularies/review",
    operation_id = "reviewSemanticConceptCandidate",
    request_body = ReviewConceptCandidateRequestDto,
    responses(
        (status = 200, body = SemanticVocabularyDto),
        (status = 400, body = ApplicationErrorDto),
        (status = 403, body = ApplicationErrorDto)
    )
)]
pub(crate) async fn review(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<ReviewConceptCandidateRequestDto>,
) -> Result<Json<SemanticVocabularyDto>, ApiError> {
    state
        .service
        .review_semantic_concept_candidate(state.semantic_access(), request)
        .await
        .map(Json)
        .map_err(|error| ApiError::new(error, extract_request_id(&request_id)))
}

#[utoipa::path(
    post,
    path = "/api/v1/semantic/vocabularies/delete",
    operation_id = "deleteSemanticVocabulary",
    request_body = DeleteSemanticVocabularyRequestDto,
    responses(
        (status = 200, body = DeleteSemanticVocabularyImpactDto),
        (status = 400, body = ApplicationErrorDto),
        (status = 403, body = ApplicationErrorDto)
    )
)]
pub(crate) async fn delete(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<DeleteSemanticVocabularyRequestDto>,
) -> Result<Json<DeleteSemanticVocabularyImpactDto>, ApiError> {
    state
        .service
        .delete_semantic_vocabulary(state.semantic_access(), request)
        .await
        .map(Json)
        .map_err(|error| ApiError::new(error, extract_request_id(&request_id)))
}
