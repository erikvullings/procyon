//! `GET /api/v1/volumes` (task 0144).

use axum::Json;
use axum::extract::{Extension, State};
use fm_transport_dto::{ApplicationErrorDto, VolumeDto};
use tower_http::request_id::RequestId;

use crate::error::{ApiError, extract_request_id};
use crate::state::AppState;

/// Lists currently mounted local/removable/disk-image volumes.
#[utoipa::path(
    get,
    path = "/api/v1/volumes",
    operation_id = "getVolumes",
    responses(
        (status = 200, description = "Discovered mounted volumes", body = [VolumeDto]),
        (status = 502, description = "Platform discovery failed", body = ApplicationErrorDto),
    )
)]
pub(crate) async fn get_volumes(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
) -> Result<Json<Vec<VolumeDto>>, ApiError> {
    state
        .service
        .volumes()
        .await
        .map(Json)
        .map_err(|error| ApiError::new(error, extract_request_id(&request_id)))
}
