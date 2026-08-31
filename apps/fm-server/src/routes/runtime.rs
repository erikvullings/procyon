//! `GET /api/v1/runtime` (spec §21).

use axum::Json;
use axum::extract::State;
use fm_transport_dto::RuntimeCapabilitiesDto;

use crate::state::AppState;

/// Reports the capabilities available for the current runtime and platform.
#[utoipa::path(
    get,
    path = "/api/v1/runtime",
    operation_id = "getRuntimeCapabilities",
    responses((status = 200, description = "Current runtime capabilities", body = RuntimeCapabilitiesDto))
)]
pub(crate) async fn get_runtime_capabilities(
    State(state): State<AppState>,
) -> Json<RuntimeCapabilitiesDto> {
    Json(state.service.runtime_capabilities())
}
