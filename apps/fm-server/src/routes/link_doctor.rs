//! Thin REST adapter for the broken-link doctor (task 0266).

use axum::{
    Json,
    extract::{Extension, Path, Query, State},
    http::StatusCode,
};
use fm_transport_dto::{
    ApplicationErrorDto, FindLinkReplacementsRequestDto, FindLinkReplacementsResponseDto,
    LinkDoctorPageDto, StartLinkDoctorScanRequestDto, StartLinkDoctorScanResponseDto,
};
use serde::Deserialize;
use tower_http::request_id::RequestId;
use uuid::Uuid;

use crate::{
    error::{ApiError, extract_request_id, require_within_roots},
    state::AppState,
};

/// Paging controls for the broken-link result endpoint.
#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LinkDoctorPageQuery {
    /// Zero-based offset into the result set.
    offset: Option<u64>,
    /// Page size, clamped to 1 through 500.
    limit: Option<u16>,
}

#[utoipa::path(
    post,
    path = "/api/v1/link-doctor-scans",
    operation_id = "startLinkDoctorScan",
    request_body = StartLinkDoctorScanRequestDto,
    responses(
        (status = 201, body = StartLinkDoctorScanResponseDto),
        (status = 400, body = ApplicationErrorDto),
        (status = 403, body = ApplicationErrorDto)
    )
)]
pub(crate) async fn start_link_doctor_scan(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<StartLinkDoctorScanRequestDto>,
) -> Result<(StatusCode, Json<StartLinkDoctorScanResponseDto>), ApiError> {
    let correlation_id = extract_request_id(&request_id);
    for root in &request.roots {
        require_within_roots(root, &state.accessible_roots, correlation_id)?;
    }
    state
        .service
        .start_link_doctor_scan(request)
        .map(|response| (StatusCode::CREATED, Json(response)))
        .map_err(|error| ApiError::new(error, correlation_id))
}

#[utoipa::path(
    get,
    path = "/api/v1/link-doctor-scans/{scanId}",
    operation_id = "getLinkDoctorScan",
    params(("scanId" = Uuid, Path), LinkDoctorPageQuery),
    responses(
        (status = 200, body = LinkDoctorPageDto),
        (status = 404, body = ApplicationErrorDto)
    )
)]
pub(crate) async fn get_link_doctor_scan(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(scan_id): Path<Uuid>,
    Query(query): Query<LinkDoctorPageQuery>,
) -> Result<Json<LinkDoctorPageDto>, ApiError> {
    state
        .service
        .get_link_doctor_page(
            scan_id,
            query.offset.unwrap_or(0),
            query.limit.unwrap_or(200),
        )
        .map(Json)
        .map_err(|error| ApiError::new(error, extract_request_id(&request_id)))
}

#[utoipa::path(
    post,
    path = "/api/v1/link-doctor-scans/{scanId}/cancel",
    operation_id = "cancelLinkDoctorScan",
    params(("scanId" = Uuid, Path)),
    responses(
        (status = 204),
        (status = 404, body = ApplicationErrorDto)
    )
)]
pub(crate) async fn cancel_link_doctor_scan(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(scan_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    state
        .service
        .cancel_link_doctor_scan(scan_id)
        .map(|()| StatusCode::NO_CONTENT)
        .map_err(|error| ApiError::new(error, extract_request_id(&request_id)))
}

#[utoipa::path(
    post,
    path = "/api/v1/link-doctor-replacements",
    operation_id = "findLinkReplacements",
    request_body = FindLinkReplacementsRequestDto,
    responses(
        (status = 200, body = FindLinkReplacementsResponseDto),
        (status = 400, body = ApplicationErrorDto),
        (status = 403, body = ApplicationErrorDto)
    )
)]
pub(crate) async fn find_link_replacements(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<FindLinkReplacementsRequestDto>,
) -> Result<Json<FindLinkReplacementsResponseDto>, ApiError> {
    let correlation_id = extract_request_id(&request_id);
    for root in &request.roots {
        require_within_roots(root, &state.accessible_roots, correlation_id)?;
    }
    state
        .service
        .find_link_replacements(request)
        .await
        .map(Json)
        .map_err(|error| ApiError::new(error, correlation_id))
}
