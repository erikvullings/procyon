//! `GET /api/v1/system-locations` (task 0101).

use axum::Json;
use axum::extract::{Extension, State};
use fm_transport_dto::{ApplicationErrorDto, SystemLocationDto};
use tower_http::request_id::RequestId;

use crate::error::{ApiError, extract_request_id};
use crate::state::AppState;

/// Lists currently reachable OS-managed filesystem locations.
#[utoipa::path(
    get,
    path = "/api/v1/system-locations",
    operation_id = "getSystemLocations",
    responses(
        (status = 200, description = "Discovered system locations", body = [SystemLocationDto]),
        (status = 502, description = "Platform discovery failed", body = ApplicationErrorDto),
    )
)]
pub(crate) async fn get_system_locations(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
) -> Result<Json<Vec<SystemLocationDto>>, ApiError> {
    state
        .service
        .system_locations()
        .await
        .map(Json)
        .map_err(|error| ApiError::new(error, extract_request_id(&request_id)))
}
