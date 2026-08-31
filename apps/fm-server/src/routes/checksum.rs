//! Thin REST adapter for checksum calculation, checksum-file verification
//! and duplicate detection (spec §16 milestone 5, §18, §37, task 0077).

use crate::{
    error::{ApiError, extract_request_id},
    state::AppState,
};
use axum::{
    Json,
    extract::{Extension, Path, Query, State},
    http::StatusCode,
};
use fm_transport_dto::{
    ApplicationErrorDto, ChecksumFileDto, ChecksumPageDto, DuplicatePageDto,
    RenderChecksumFileRequestDto, SaveChecksumFileRequestDto, SaveChecksumFileResponseDto,
    StartChecksumRequestDto, StartChecksumResponseDto, StartDuplicateScanRequestDto,
    StartDuplicateScanResponseDto, VerificationReportDto, VerifyChecksumFileRequestDto,
};
use serde::Deserialize;
use tower_http::request_id::RequestId;
use uuid::Uuid;

/// Paging controls shared by the checksum and duplicate result endpoints.
#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResultPageQuery {
    /// Zero-based offset into the result set.
    offset: Option<u64>,
    /// Page size, clamped to 1 through 500.
    limit: Option<u16>,
}

#[utoipa::path(
    post,
    path = "/api/v1/checksums",
    operation_id = "startChecksums",
    request_body = StartChecksumRequestDto,
    responses(
        (status = 201, body = StartChecksumResponseDto),
        (status = 400, body = ApplicationErrorDto)
    )
)]
pub(crate) async fn start_checksums(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<StartChecksumRequestDto>,
) -> Result<(StatusCode, Json<StartChecksumResponseDto>), ApiError> {
    let correlation_id = extract_request_id(&request_id);
    state
        .service
        .start_checksums(request)
        .map(|response| (StatusCode::CREATED, Json(response)))
        .map_err(|error| ApiError::new(error, correlation_id))
}

#[utoipa::path(
    get,
    path = "/api/v1/checksums/{jobId}",
    operation_id = "getChecksums",
    params(("jobId" = Uuid, Path), ResultPageQuery),
    responses(
        (status = 200, body = ChecksumPageDto),
        (status = 404, body = ApplicationErrorDto)
    )
)]
pub(crate) async fn get_checksums(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(job_id): Path<Uuid>,
    Query(query): Query<ResultPageQuery>,
) -> Result<Json<ChecksumPageDto>, ApiError> {
    state
        .service
        .get_checksum_page(
            job_id,
            query.offset.unwrap_or(0),
            query.limit.unwrap_or(200),
        )
        .map(Json)
        .map_err(|error| ApiError::new(error, extract_request_id(&request_id)))
}

#[utoipa::path(
    post,
    path = "/api/v1/checksums/{jobId}/cancel",
    operation_id = "cancelChecksums",
    params(("jobId" = Uuid, Path)),
    responses(
        (status = 204),
        (status = 404, body = ApplicationErrorDto)
    )
)]
pub(crate) async fn cancel_checksums(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(job_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    state
        .service
        .cancel_checksums(job_id)
        .map(|()| StatusCode::NO_CONTENT)
        .map_err(|error| ApiError::new(error, extract_request_id(&request_id)))
}

#[utoipa::path(
    post,
    path = "/api/v1/checksums/{jobId}/checksum-file",
    operation_id = "renderChecksumFile",
    params(("jobId" = Uuid, Path)),
    request_body = RenderChecksumFileRequestDto,
    responses(
        (status = 200, body = ChecksumFileDto),
        (status = 404, body = ApplicationErrorDto)
    )
)]
pub(crate) async fn render_checksum_file(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(job_id): Path<Uuid>,
    Json(request): Json<RenderChecksumFileRequestDto>,
) -> Result<Json<ChecksumFileDto>, ApiError> {
    state
        .service
        .render_checksum_file(job_id, request)
        .map(Json)
        .map_err(|error| ApiError::new(error, extract_request_id(&request_id)))
}

#[utoipa::path(
    post,
    path = "/api/v1/checksums/{jobId}/save",
    operation_id = "saveChecksumFile",
    params(("jobId" = Uuid, Path)),
    request_body = SaveChecksumFileRequestDto,
    responses(
        (status = 201, body = SaveChecksumFileResponseDto),
        (status = 400, body = ApplicationErrorDto),
        (status = 404, body = ApplicationErrorDto)
    )
)]
pub(crate) async fn save_checksum_file(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(job_id): Path<Uuid>,
    Json(request): Json<SaveChecksumFileRequestDto>,
) -> Result<(StatusCode, Json<SaveChecksumFileResponseDto>), ApiError> {
    let correlation_id = extract_request_id(&request_id);
    state
        .service
        .save_checksum_file(job_id, request)
        .await
        .map(|response| (StatusCode::CREATED, Json(response)))
        .map_err(|error| ApiError::new(error, correlation_id))
}

#[utoipa::path(
    post,
    path = "/api/v1/checksums/{jobId}/verify",
    operation_id = "verifyChecksumFile",
    params(("jobId" = Uuid, Path)),
    request_body = VerifyChecksumFileRequestDto,
    responses(
        (status = 200, body = VerificationReportDto),
        (status = 400, body = ApplicationErrorDto),
        (status = 404, body = ApplicationErrorDto)
    )
)]
pub(crate) async fn verify_checksum_file(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(job_id): Path<Uuid>,
    Json(request): Json<VerifyChecksumFileRequestDto>,
) -> Result<Json<VerificationReportDto>, ApiError> {
    state
        .service
        .verify_checksum_file(job_id, request)
        .map(Json)
        .map_err(|error| ApiError::new(error, extract_request_id(&request_id)))
}

#[utoipa::path(
    post,
    path = "/api/v1/duplicate-scans",
    operation_id = "startDuplicateScan",
    request_body = StartDuplicateScanRequestDto,
    responses(
        (status = 201, body = StartDuplicateScanResponseDto),
        (status = 400, body = ApplicationErrorDto)
    )
)]
pub(crate) async fn start_duplicate_scan(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<StartDuplicateScanRequestDto>,
) -> Result<(StatusCode, Json<StartDuplicateScanResponseDto>), ApiError> {
    let correlation_id = extract_request_id(&request_id);
    state
        .service
        .start_duplicate_scan(request)
        .map(|response| (StatusCode::CREATED, Json(response)))
        .map_err(|error| ApiError::new(error, correlation_id))
}

#[utoipa::path(
    get,
    path = "/api/v1/duplicate-scans/{scanId}",
    operation_id = "getDuplicateScan",
    params(("scanId" = Uuid, Path), ResultPageQuery),
    responses(
        (status = 200, body = DuplicatePageDto),
        (status = 404, body = ApplicationErrorDto)
    )
)]
pub(crate) async fn get_duplicate_scan(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(scan_id): Path<Uuid>,
    Query(query): Query<ResultPageQuery>,
) -> Result<Json<DuplicatePageDto>, ApiError> {
    state
        .service
        .get_duplicate_page(
            scan_id,
            query.offset.unwrap_or(0),
            query.limit.unwrap_or(200),
        )
        .map(Json)
        .map_err(|error| ApiError::new(error, extract_request_id(&request_id)))
}

#[utoipa::path(
    post,
    path = "/api/v1/duplicate-scans/{scanId}/cancel",
    operation_id = "cancelDuplicateScan",
    params(("scanId" = Uuid, Path)),
    responses(
        (status = 204),
        (status = 404, body = ApplicationErrorDto)
    )
)]
pub(crate) async fn cancel_duplicate_scan(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(scan_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    state
        .service
        .cancel_duplicate_scan(scan_id)
        .map(|()| StatusCode::NO_CONTENT)
        .map_err(|error| ApiError::new(error, extract_request_id(&request_id)))
}
