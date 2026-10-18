//! Thin REST adapter for folder snapshots and snapshot comparison (task 0268).

use axum::{
    Json,
    extract::{Extension, Path, Query, State},
    http::StatusCode,
};
use fm_transport_dto::{
    ApplicationErrorDto, SnapshotJobPageDto, StartSnapshotJobRequestDto,
    StartSnapshotJobResponseDto,
};
use serde::Deserialize;
use tower_http::request_id::RequestId;
use uuid::Uuid;

use crate::{
    error::{ApiError, extract_request_id, require_within_roots},
    state::AppState,
};

/// Paging controls for the snapshot job result endpoint.
#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SnapshotPageQuery {
    /// Zero-based offset into the result set.
    offset: Option<u64>,
    /// Page size, clamped to 1 through 500.
    limit: Option<u16>,
}

#[utoipa::path(
    post,
    path = "/api/v1/snapshot-jobs",
    operation_id = "startSnapshotJob",
    request_body = StartSnapshotJobRequestDto,
    responses(
        (status = 201, body = StartSnapshotJobResponseDto),
        (status = 403, body = ApplicationErrorDto)
    )
)]
pub(crate) async fn start_snapshot_job(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<StartSnapshotJobRequestDto>,
) -> Result<(StatusCode, Json<StartSnapshotJobResponseDto>), ApiError> {
    let correlation_id = extract_request_id(&request_id);
    let locations = match &request {
        StartSnapshotJobRequestDto::Create {
            root, destination, ..
        } => [root, destination],
        StartSnapshotJobRequestDto::Compare {
            snapshot, target, ..
        } => [snapshot, target],
    };
    for location in locations {
        require_within_roots(location, &state.accessible_roots, correlation_id)?;
    }
    let response = state.service.start_snapshot_job(request);
    Ok((StatusCode::CREATED, Json(response)))
}

#[utoipa::path(
    get,
    path = "/api/v1/snapshot-jobs/{jobId}",
    operation_id = "getSnapshotJob",
    params(("jobId" = Uuid, Path), SnapshotPageQuery),
    responses(
        (status = 200, body = SnapshotJobPageDto),
        (status = 404, body = ApplicationErrorDto)
    )
)]
pub(crate) async fn get_snapshot_job(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(job_id): Path<Uuid>,
    Query(query): Query<SnapshotPageQuery>,
) -> Result<Json<SnapshotJobPageDto>, ApiError> {
    state
        .service
        .get_snapshot_job_page(
            job_id,
            query.offset.unwrap_or(0),
            query.limit.unwrap_or(200),
        )
        .map(Json)
        .map_err(|error| ApiError::new(error, extract_request_id(&request_id)))
}

#[utoipa::path(
    post,
    path = "/api/v1/snapshot-jobs/{jobId}/cancel",
    operation_id = "cancelSnapshotJob",
    params(("jobId" = Uuid, Path)),
    responses(
        (status = 204),
        (status = 404, body = ApplicationErrorDto)
    )
)]
pub(crate) async fn cancel_snapshot_job(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(job_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    state
        .service
        .cancel_snapshot_job(job_id)
        .map(|()| StatusCode::NO_CONTENT)
        .map_err(|error| ApiError::new(error, extract_request_id(&request_id)))
}
