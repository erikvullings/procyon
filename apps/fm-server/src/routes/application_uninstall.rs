//! Application uninstall discovery REST transport (task 0148, macOS only).

use axum::Json;
use axum::extract::{Extension, State};
use fm_transport_dto::{
    ApplicationErrorDto, DiscoverApplicationUninstallCandidatesRequestDto,
    DiscoverApplicationUninstallCandidatesResponseDto, RemoveApplicationDockIconRequestDto,
    RemoveApplicationDockIconResponseDto,
};
use tower_http::request_id::RequestId;

use crate::error::{ApiError, extract_request_id};
use crate::state::AppState;

/// Scans a `.app` bundle's well-known related-file locations, for the user
/// to review before anything is deleted. Read-only: nothing is deleted by
/// this call.
#[utoipa::path(
    post,
    path = "/api/v1/applications/uninstall/discover",
    operation_id = "discoverApplicationUninstallCandidates",
    request_body = DiscoverApplicationUninstallCandidatesRequestDto,
    responses(
        (status = 200, description = "The bundle's identity and discovered related files", body = DiscoverApplicationUninstallCandidatesResponseDto),
        (status = 400, description = "The request was invalid", body = ApplicationErrorDto),
        (status = 403, description = "The location is outside an accessible root", body = ApplicationErrorDto),
        (status = 404, description = "The bundle does not exist or is not a `.app` bundle", body = ApplicationErrorDto),
        (status = 502, description = "Uninstall is unavailable, or the platform scan failed", body = ApplicationErrorDto),
    )
)]
pub(crate) async fn discover_application_uninstall_candidates(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<DiscoverApplicationUninstallCandidatesRequestDto>,
) -> Result<Json<DiscoverApplicationUninstallCandidatesResponseDto>, ApiError> {
    let request_id = extract_request_id(&request_id);
    crate::error::require_within_roots(&request.location, &state.accessible_roots, request_id)?;
    state
        .service
        .discover_application_uninstall_candidates(request)
        .map(Json)
        .map_err(|error| ApiError::new(error, request_id))
}

/// Removes a `.app` bundle's pinned Dock icon, if it has one, once the user has confirmed an
/// uninstall - a Dock icon left pointing at a trashed bundle is otherwise never cleaned up on its
/// own. Best-effort: no pinned icon is a normal `removed: false`, not an error.
#[utoipa::path(
    post,
    path = "/api/v1/applications/uninstall/remove-dock-icon",
    operation_id = "removeApplicationDockIcon",
    request_body = RemoveApplicationDockIconRequestDto,
    responses(
        (status = 200, description = "Whether a pinned Dock icon was found and removed", body = RemoveApplicationDockIconResponseDto),
        (status = 400, description = "The request was invalid", body = ApplicationErrorDto),
        (status = 403, description = "The location is outside an accessible root", body = ApplicationErrorDto),
        (status = 502, description = "The platform failed to update Dock preferences", body = ApplicationErrorDto),
    )
)]
pub(crate) async fn remove_application_dock_icon(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<RemoveApplicationDockIconRequestDto>,
) -> Result<Json<RemoveApplicationDockIconResponseDto>, ApiError> {
    let request_id = extract_request_id(&request_id);
    crate::error::require_within_roots(&request.location, &state.accessible_roots, request_id)?;
    state
        .service
        .remove_application_dock_icon(request)
        .map(Json)
        .map_err(|error| ApiError::new(error, request_id))
}
