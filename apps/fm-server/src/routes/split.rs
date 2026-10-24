//! Split-set detection REST transport (task 0257). Splitting and combining themselves are
//! `splitFile` / `combineFiles` operations started through the operation routes.

use axum::Json;
use axum::extract::{Extension, State};
use fm_transport_dto::{
    ApplicationErrorDto, InspectSplitSetRequestDto, InspectSplitSetResponseDto,
};
use tower_http::request_id::RequestId;

use crate::error::{ApiError, extract_request_id};
use crate::state::AppState;

/// Detects the split set a selection of part files belongs to and whether it can be joined.
#[utoipa::path(
    post,
    path = "/api/v1/split/inspect",
    operation_id = "inspectSplitSet",
    request_body = InspectSplitSetRequestDto,
    responses(
        (status = 200, description = "The detected set, or the reason it cannot be joined", body = InspectSplitSetResponseDto),
        (status = 403, description = "A part is outside the accessible roots", body = ApplicationErrorDto),
        (status = 404, description = "A part does not exist", body = ApplicationErrorDto),
    )
)]
pub(crate) async fn inspect_split_set(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<InspectSplitSetRequestDto>,
) -> Result<Json<InspectSplitSetResponseDto>, ApiError> {
    let request_id = extract_request_id(&request_id);
    for location in &request.locations {
        crate::error::require_within_roots(location, &state.accessible_roots, request_id)?;
    }
    state
        .service
        .inspect_split_set(request)
        .await
        .map(Json)
        .map_err(|error| ApiError::new(error, request_id))
}
