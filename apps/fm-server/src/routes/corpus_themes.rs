//! Thin HTTP adapter for corpus themes.

use axum::Json;
use axum::extract::{Extension, State};
use fm_transport_dto::{
    AnalyzeCorpusThemesRequestDto, ApplicationErrorDto, CorpusThemeSourcesDto,
    CorpusThemeStatusDto, CorpusThemesResponseDto, GetCorpusThemeSourcesRequestDto,
    GetCorpusThemeStatusRequestDto,
};
use tower_http::request_id::RequestId;

use crate::error::{ApiError, extract_request_id};
use crate::state::AppState;

#[utoipa::path(
    post,
    path = "/api/v1/semantic/themes/status",
    operation_id = "getCorpusThemeStatus",
    request_body = GetCorpusThemeStatusRequestDto,
    responses(
        (status = 200, body = CorpusThemeStatusDto),
        (status = 400, body = ApplicationErrorDto),
        (status = 403, body = ApplicationErrorDto),
        (status = 503, body = ApplicationErrorDto)
    )
)]
pub(crate) async fn get_corpus_theme_status(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<GetCorpusThemeStatusRequestDto>,
) -> Result<Json<CorpusThemeStatusDto>, ApiError> {
    state
        .service
        .get_corpus_theme_status(state.semantic_access(), request)
        .await
        .map(Json)
        .map_err(|error| ApiError::new(error, extract_request_id(&request_id)))
}

#[utoipa::path(
    post,
    path = "/api/v1/semantic/themes/analyze",
    operation_id = "analyzeCorpusThemes",
    request_body = AnalyzeCorpusThemesRequestDto,
    responses(
        (status = 200, body = CorpusThemesResponseDto),
        (status = 400, body = ApplicationErrorDto),
        (status = 403, body = ApplicationErrorDto),
        (status = 503, body = ApplicationErrorDto)
    )
)]
pub(crate) async fn analyze_corpus_themes(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<AnalyzeCorpusThemesRequestDto>,
) -> Result<Json<CorpusThemesResponseDto>, ApiError> {
    state
        .service
        .analyze_corpus_themes(state.semantic_access(), request)
        .await
        .map(Json)
        .map_err(|error| ApiError::new(error, extract_request_id(&request_id)))
}

#[utoipa::path(
    post,
    path = "/api/v1/semantic/themes/sources",
    operation_id = "getCorpusThemeSources",
    request_body = GetCorpusThemeSourcesRequestDto,
    responses(
        (status = 200, body = CorpusThemeSourcesDto),
        (status = 400, body = ApplicationErrorDto),
        (status = 403, body = ApplicationErrorDto),
        (status = 503, body = ApplicationErrorDto)
    )
)]
pub(crate) async fn get_corpus_theme_sources(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<GetCorpusThemeSourcesRequestDto>,
) -> Result<Json<CorpusThemeSourcesDto>, ApiError> {
    state
        .service
        .get_corpus_theme_sources(state.semantic_access(), request)
        .await
        .map(Json)
        .map_err(|error| ApiError::new(error, extract_request_id(&request_id)))
}
