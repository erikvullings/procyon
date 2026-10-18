//! Thin REST adapter for the duplicate and near-duplicate music finder (task 0265).

use axum::{
    Json,
    extract::{Extension, Path, Query, State},
    http::StatusCode,
};
use fm_transport_dto::{
    ApplicationErrorDto, MusicDuplicatePageDto, StartMusicDuplicateScanRequestDto,
    StartMusicDuplicateScanResponseDto,
};
use serde::Deserialize;
use tower_http::request_id::RequestId;
use uuid::Uuid;

use crate::{
    error::{ApiError, extract_request_id, require_within_roots},
    state::AppState,
};

/// Paging controls for the music duplicate result endpoint.
#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MusicPageQuery {
    /// Zero-based offset into the result set.
    offset: Option<u64>,
    /// Page size, clamped to 1 through 500.
    limit: Option<u16>,
}

#[utoipa::path(
    post,
    path = "/api/v1/music-duplicate-scans",
    operation_id = "startMusicDuplicateScan",
    request_body = StartMusicDuplicateScanRequestDto,
    responses(
        (status = 201, body = StartMusicDuplicateScanResponseDto),
        (status = 400, body = ApplicationErrorDto),
        (status = 403, body = ApplicationErrorDto)
    )
)]
pub(crate) async fn start_music_duplicate_scan(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<StartMusicDuplicateScanRequestDto>,
) -> Result<(StatusCode, Json<StartMusicDuplicateScanResponseDto>), ApiError> {
    let correlation_id = extract_request_id(&request_id);
    for root in &request.roots {
        require_within_roots(root, &state.accessible_roots, correlation_id)?;
    }
    state
        .service
        .start_music_duplicate_scan(request)
        .map(|response| (StatusCode::CREATED, Json(response)))
        .map_err(|error| ApiError::new(error, correlation_id))
}

#[utoipa::path(
    get,
    path = "/api/v1/music-duplicate-scans/{scanId}",
    operation_id = "getMusicDuplicateScan",
    params(("scanId" = Uuid, Path), MusicPageQuery),
    responses(
        (status = 200, body = MusicDuplicatePageDto),
        (status = 404, body = ApplicationErrorDto)
    )
)]
pub(crate) async fn get_music_duplicate_scan(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(scan_id): Path<Uuid>,
    Query(query): Query<MusicPageQuery>,
) -> Result<Json<MusicDuplicatePageDto>, ApiError> {
    state
        .service
        .get_music_duplicate_page(
            scan_id,
            query.offset.unwrap_or(0),
            query.limit.unwrap_or(200),
        )
        .map(Json)
        .map_err(|error| ApiError::new(error, extract_request_id(&request_id)))
}

#[utoipa::path(
    post,
    path = "/api/v1/music-duplicate-scans/{scanId}/cancel",
    operation_id = "cancelMusicDuplicateScan",
    params(("scanId" = Uuid, Path)),
    responses(
        (status = 204),
        (status = 404, body = ApplicationErrorDto)
    )
)]
pub(crate) async fn cancel_music_duplicate_scan(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(scan_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    state
        .service
        .cancel_music_duplicate_scan(scan_id)
        .map(|()| StatusCode::NO_CONTENT)
        .map_err(|error| ApiError::new(error, extract_request_id(&request_id)))
}
