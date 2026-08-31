//! `GET /api/v1/health` (spec §33 step 2).

use axum::Json;
use fm_transport_dto::{HealthDto, HealthStatusDto};

/// Reports that the backend process is running and able to serve requests.
#[utoipa::path(
    get,
    path = "/api/v1/health",
    operation_id = "getHealth",
    responses((status = 200, description = "The backend is healthy", body = HealthDto))
)]
pub(crate) async fn get_health() -> Json<HealthDto> {
    Json(HealthDto {
        status: HealthStatusDto::Ok,
    })
}
