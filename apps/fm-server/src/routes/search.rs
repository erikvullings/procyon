//! Thin REST adapter for starting and cancelling recursive filesystem
//! search (spec §24, task 0068).

use crate::{
    error::{ApiError, extract_request_id},
    state::AppState,
};
use axum::{
    Json,
    extract::{Extension, Path, State},
    http::StatusCode,
};
use fm_transport_dto::{ApplicationErrorDto, StartSearchRequestDto, StartSearchResponseDto};
use tower_http::request_id::RequestId;
use uuid::Uuid;

#[utoipa::path(
    post,
    path = "/api/v1/search",
    operation_id = "startSearch",
    request_body = StartSearchRequestDto,
    responses(
        (status = 201, body = StartSearchResponseDto),
        (status = 400, body = ApplicationErrorDto)
    )
)]
pub(crate) async fn start_search(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<StartSearchRequestDto>,
) -> Result<(StatusCode, Json<StartSearchResponseDto>), ApiError> {
    let correlation_id = extract_request_id(&request_id);
    for root in &request.roots {
        crate::error::require_within_roots(root, &state.accessible_roots, correlation_id)?;
    }
    state
        .service
        .start_search(request)
        .map(|response| (StatusCode::CREATED, Json(response)))
        .map_err(|error| ApiError::new(error, correlation_id))
}

#[utoipa::path(
    post,
    path = "/api/v1/search/{searchId}/cancel",
    operation_id = "cancelSearch",
    params(("searchId" = Uuid, Path)),
    responses(
        (status = 204),
        (status = 404, body = ApplicationErrorDto)
    )
)]
pub(crate) async fn cancel_search(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(search_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    state
        .service
        .cancel_search(search_id)
        .map(|()| StatusCode::NO_CONTENT)
        .map_err(|error| ApiError::new(error, extract_request_id(&request_id)))
}
