//! Thin application settings REST adapter (specification §26).

use axum::Json;
use axum::extract::{Extension, State};
use fm_transport_dto::{ApplicationErrorDto, SettingsDto};
use tower_http::request_id::RequestId;

use crate::error::{ApiError, extract_request_id};
use crate::state::AppState;

/// Returns the complete application-wide settings document.
#[utoipa::path(
    get,
    path = "/api/v1/settings",
    operation_id = "getSettings",
    responses((status = 200, description = "Current settings", body = SettingsDto))
)]
pub(crate) async fn get_settings(State(state): State<AppState>) -> Json<SettingsDto> {
    Json(state.service.get_settings())
}

/// Atomically replaces the complete application-wide settings document.
#[utoipa::path(
    put,
    path = "/api/v1/settings",
    operation_id = "updateSettings",
    request_body = SettingsDto,
    responses(
        (status = 200, description = "Persisted settings", body = SettingsDto),
        (status = 500, description = "Settings could not be persisted", body = ApplicationErrorDto),
    )
)]
pub(crate) async fn update_settings(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(settings): Json<SettingsDto>,
) -> Result<Json<SettingsDto>, ApiError> {
    state
        .service
        .update_settings(settings)
        .map(Json)
        .map_err(|error| ApiError::new(error, extract_request_id(&request_id)))
}
