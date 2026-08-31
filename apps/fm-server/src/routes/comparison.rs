//! Thin REST adapter for directory comparison and basic synchronization
//! (spec §16 milestone 5, §37, task 0075).

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
    ApplicationErrorDto, ApplySyncPlanRequestDto, ApplySyncPlanResponseDto, ComparisonPageDto,
    GenerateSyncPlanRequestDto, StartComparisonRequestDto, StartComparisonResponseDto, SyncPlanDto,
};
use serde::Deserialize;
use tower_http::request_id::RequestId;
use uuid::Uuid;

#[utoipa::path(
    post,
    path = "/api/v1/comparisons",
    operation_id = "startComparison",
    request_body = StartComparisonRequestDto,
    responses(
        (status = 201, body = StartComparisonResponseDto),
        (status = 400, body = ApplicationErrorDto)
    )
)]
pub(crate) async fn start_comparison(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<StartComparisonRequestDto>,
) -> Result<(StatusCode, Json<StartComparisonResponseDto>), ApiError> {
    let correlation_id = extract_request_id(&request_id);
    state
        .service
        .start_comparison(request)
        .map(|response| (StatusCode::CREATED, Json(response)))
        .map_err(|error| ApiError::new(error, correlation_id))
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ComparisonPageQuery {
    /// Zero-based entry offset, into the (possibly filtered) result set.
    offset: Option<u64>,
    /// Page size, clamped to 1 through 500.
    limit: Option<u16>,
    /// Restrict the page to entries that are not identical.
    #[serde(default)]
    differences_only: bool,
}

#[utoipa::path(
    get,
    path = "/api/v1/comparisons/{comparisonId}",
    operation_id = "getComparison",
    params(("comparisonId" = Uuid, Path), ComparisonPageQuery),
    responses(
        (status = 200, body = ComparisonPageDto),
        (status = 404, body = ApplicationErrorDto)
    )
)]
pub(crate) async fn get_comparison(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(comparison_id): Path<Uuid>,
    Query(query): Query<ComparisonPageQuery>,
) -> Result<Json<ComparisonPageDto>, ApiError> {
    state
        .service
        .get_comparison_page(
            comparison_id,
            query.offset.unwrap_or(0),
            query.limit.unwrap_or(200),
            query.differences_only,
        )
        .map(Json)
        .map_err(|error| ApiError::new(error, extract_request_id(&request_id)))
}

#[utoipa::path(
    post,
    path = "/api/v1/comparisons/{comparisonId}/cancel",
    operation_id = "cancelComparison",
    params(("comparisonId" = Uuid, Path)),
    responses(
        (status = 204),
        (status = 404, body = ApplicationErrorDto)
    )
)]
pub(crate) async fn cancel_comparison(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(comparison_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    state
        .service
        .cancel_comparison(comparison_id)
        .map(|()| StatusCode::NO_CONTENT)
        .map_err(|error| ApiError::new(error, extract_request_id(&request_id)))
}

#[utoipa::path(
    post,
    path = "/api/v1/comparisons/{comparisonId}/sync-plan",
    operation_id = "generateSyncPlan",
    params(("comparisonId" = Uuid, Path)),
    request_body = GenerateSyncPlanRequestDto,
    responses(
        (status = 200, body = SyncPlanDto),
        (status = 404, body = ApplicationErrorDto)
    )
)]
pub(crate) async fn generate_sync_plan(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(comparison_id): Path<Uuid>,
    Json(request): Json<GenerateSyncPlanRequestDto>,
) -> Result<Json<SyncPlanDto>, ApiError> {
    state
        .service
        .generate_sync_plan(comparison_id, request)
        .map(Json)
        .map_err(|error| ApiError::new(error, extract_request_id(&request_id)))
}

#[utoipa::path(
    post,
    path = "/api/v1/comparisons/{comparisonId}/apply-sync-plan",
    operation_id = "applySyncPlan",
    params(("comparisonId" = Uuid, Path)),
    request_body = ApplySyncPlanRequestDto,
    responses(
        (status = 201, body = ApplySyncPlanResponseDto),
        (status = 400, body = ApplicationErrorDto),
        (status = 404, body = ApplicationErrorDto)
    )
)]
pub(crate) async fn apply_sync_plan(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(comparison_id): Path<Uuid>,
    Json(request): Json<ApplySyncPlanRequestDto>,
) -> Result<(StatusCode, Json<ApplySyncPlanResponseDto>), ApiError> {
    let correlation_id = extract_request_id(&request_id);
    state
        .service
        .apply_sync_plan(comparison_id, request)
        .map(|response| (StatusCode::CREATED, Json(response)))
        .map_err(|error| ApiError::new(error, correlation_id))
}
